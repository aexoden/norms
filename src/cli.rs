// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Command-line interface.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::checks;
use crate::context::ProjectContext;
use crate::models::{Language, Status};
use crate::report::{CheckResult, Report};

#[derive(Parser)]
#[command(name = "norms", about = "Verify and enforce project coding standards.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Verify the project at the given path is correctly configured.
    Verify {
        /// Path to the project to verify.
        path: PathBuf,

        /// Output results as JSON.
        #[arg(long = "json")]
        output_json: bool,
    },
}

/// JSON output structure.
#[derive(Serialize)]
struct JsonOutput {
    path: String,
    languages: Vec<String>,
    components: Vec<JsonComponent>,
    summary: JsonSummary,
    results: Vec<JsonResult>,
}

#[derive(Serialize)]
struct JsonComponent {
    language: String,
    path: String,
}

#[derive(Serialize)]
struct JsonSummary {
    passed: usize,
    failed: usize,
    warnings: usize,
    skipped: usize,
}

#[derive(Serialize)]
struct JsonResult {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    component: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
}

/// The lowercase name of a language in JSON output.
fn language_id(language: Language) -> String {
    match language {
        Language::Cpp => "cpp".into(),
        Language::Python => "python".into(),
        Language::Rust => "rust".into(),
        Language::TypeScript => "typescript".into(),
    }
}

fn json_output(report: &Report) -> anyhow::Result<String> {
    let output = JsonOutput {
        path: report
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| report.project_path.clone())
            .display()
            .to_string(),
        languages: report.languages().into_iter().map(language_id).collect(),
        components: report
            .components
            .iter()
            .map(|component| JsonComponent {
                language: language_id(component.language),
                path: component.display_path(),
            })
            .collect(),
        summary: JsonSummary {
            passed: report.passed(),
            failed: report.failed(),
            warnings: report.warnings(),
            skipped: report.skipped(),
        },
        results: report
            .results
            .iter()
            .map(|r: &CheckResult| JsonResult {
                id: r.id.clone(),
                name: r.name.clone(),
                component: r.component.clone(),
                status: r.status.to_string().to_lowercase(),
                message: r.message.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&output).context("Failed to serialize JSON output")
}

fn verify(path: PathBuf, output_json: bool) -> anyhow::Result<ExitCode> {
    if !path.is_dir() {
        eprintln!("Error: '{}' is not a valid directory.", path.display());
        return Ok(ExitCode::FAILURE);
    }

    let ctx = ProjectContext::new(path.clone());
    let components = ctx.components().to_vec();
    let mut report = Report::new(path, components);

    checks::run_all_checks(&ctx, &mut report);

    if output_json {
        println!("{}", json_output(&report)?);
    } else {
        report.print();
    }

    if report.results.iter().any(|r| r.status == Status::Fail) {
        return Ok(ExitCode::FAILURE);
    }

    Ok(ExitCode::SUCCESS)
}

/// Entry point for the CLI.
#[must_use]
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Verify { path, output_json } => verify(path, output_json).unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Component;

    #[test]
    fn json_output_includes_check_ids() {
        let mut report = Report::new(PathBuf::new(), Vec::new());
        report.should("readme-exists", "README.md", false);

        let json: serde_json::Value = serde_json::from_str(&json_output(&report).unwrap()).unwrap();
        let result = json.pointer("/results/0").unwrap();
        assert_eq!(result.get("id").unwrap(), "readme-exists");
        assert_eq!(result.get("name").unwrap(), "README.md");
        assert_eq!(result.get("status").unwrap(), "warn");
        assert_eq!(json.pointer("/summary/skipped").unwrap(), 0);
    }

    #[test]
    fn json_output_includes_components() {
        let frontend = Component {
            language: Language::TypeScript,
            path: PathBuf::from("frontend"),
        };
        let mut report = Report::new(PathBuf::new(), vec![frontend.clone()]);
        report.should("readme-exists", "README.md", true);
        report.with_component(&frontend, |report| {
            report.should("package-json-name", "package.json: has name", true);
        });

        let json: serde_json::Value = serde_json::from_str(&json_output(&report).unwrap()).unwrap();
        assert_eq!(
            json.get("components").unwrap(),
            &serde_json::json!([{ "language": "typescript", "path": "frontend" }])
        );
        assert_eq!(json.pointer("/languages/0").unwrap(), "typescript");
        assert!(json.pointer("/results/0/component").is_none());
        assert_eq!(json.pointer("/results/1/component").unwrap(), "frontend");
    }
}

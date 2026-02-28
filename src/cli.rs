// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Command-line interface.

use std::path::PathBuf;
use std::process;

use anyhow::Context;
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

/// JSON output structure
#[derive(Serialize)]
struct JsonOutput {
    path: String,
    languages: Vec<String>,
    summary: JsonSummary,
    results: Vec<JsonResult>,
}

#[derive(Serialize)]
struct JsonSummary {
    passed: usize,
    failed: usize,
    warnings: usize,
}

#[derive(Serialize)]
struct JsonResult {
    name: String,
    status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
}

fn json_output(report: &Report) -> anyhow::Result<String> {
    let output = JsonOutput {
        path: report
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| report.project_path.clone())
            .display()
            .to_string(),
        languages: {
            let mut langs: Vec<String> = report
                .languages
                .iter()
                .map(|l| match l {
                    Language::Cpp => "cpp".into(),
                    Language::Python => "python".into(),
                    Language::Rust => "rust".into(),
                    Language::TypeScript => "typescript".into(),
                })
                .collect();
            langs.sort();
            langs
        },
        summary: JsonSummary {
            passed: report.passed(),
            failed: report.failed(),
            warnings: report.warnings(),
        },
        results: report
            .results
            .iter()
            .map(|r: &CheckResult| JsonResult {
                name: r.name.clone(),
                status: r.status.to_string().to_lowercase(),
                message: r.message.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&output).context("Failed to serialize JSON output")
}

fn verify(path: PathBuf, output_json: bool) -> anyhow::Result<()> {
    if !path.is_dir() {
        eprintln!("Error: '{}' is not a valid directory.", path.display());
        process::exit(1);
    }

    let ctx = ProjectContext::new(path.clone());
    let languages = ctx.languages().clone();
    let mut report = Report::new(path, languages);

    checks::run_all_checks(&ctx, &mut report);

    if output_json {
        println!("{}", json_output(&report)?);
    } else {
        report.print();
    }

    if report.results.iter().any(|r| r.status == Status::Fail) {
        process::exit(1);
    }

    Ok(())
}

/// Entry point for the CLI.
pub fn run() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Verify { path, output_json } => {
            if let Err(e) = verify(path, output_json) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
    }
}

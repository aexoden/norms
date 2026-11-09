// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Command-line interface.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::checks;
use crate::commit_msg::{self, MAX_LINE_LENGTH};
use crate::context::ProjectContext;
use crate::git;
use crate::models::Language;
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

        /// Exit with failure if any check warns, not only if one fails.
        #[arg(long)]
        deny_warnings: bool,
    },

    /// Check a commit message's line lengths, for a commit-msg hook.
    CommitMsg {
        /// Path to the file holding the commit message.
        file: PathBuf,
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

/// Whether a report fails verification: if any check failed, or, with
/// `deny_warnings`, if any check warned.
fn report_fails(report: &Report, deny_warnings: bool) -> bool {
    report.failed() > 0 || (deny_warnings && report.warnings() > 0)
}

fn verify(path: PathBuf, output_json: bool, deny_warnings: bool) -> anyhow::Result<ExitCode> {
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

    if !report_fails(&report, deny_warnings) {
        return Ok(ExitCode::SUCCESS);
    }
    if report.failed() == 0 {
        let warnings = report.warnings();
        let (noun, failure) = if warnings == 1 {
            ("warning", "a failure")
        } else {
            ("warnings", "failures")
        };
        eprintln!("Error: {warnings} {noun}, treated as {failure} by --deny-warnings.");
    }
    Ok(ExitCode::FAILURE)
}

/// Describes each line of a commit message that is too long, as
/// `<file>:<line>: <problem>`. See [`commit_msg::long_lines`] for `comment`
/// and `edited`.
fn commit_msg_problems(file: &Path, message: &str, comment: &str, edited: bool) -> Vec<String> {
    commit_msg::long_lines(message, comment, edited)
        .into_iter()
        .map(|line| {
            let what = if line.subject { "subject" } else { "body line" };
            format!(
                "{}:{}: {what} is {} characters long, over {MAX_LINE_LENGTH}",
                file.display(),
                line.number,
                line.length
            )
        })
        .collect()
}

fn commit_msg(file: &Path) -> anyhow::Result<ExitCode> {
    let message = fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
    // git runs a commit-msg hook with `GIT_EDITOR=:` when it opens no editor, as for `git commit -m`, and then keeps
    // comment lines.
    let edited = env::var_os("GIT_EDITOR").is_none_or(|editor| editor != ":");
    let problems = commit_msg_problems(
        file,
        &String::from_utf8_lossy(&message),
        &git::comment_string(),
        edited,
    );
    if problems.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    for problem in problems {
        eprintln!("{problem}");
    }
    eprintln!(
        "Keep the subject within {MAX_LINE_LENGTH} characters (aiming for 50) and wrap the body at \
         {MAX_LINE_LENGTH}; body lines with URLs are exempt."
    );
    Ok(ExitCode::FAILURE)
}

/// Entry point for the CLI.
#[must_use]
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Verify {
            path,
            output_json,
            deny_warnings,
        } => verify(path, output_json, deny_warnings),
        Commands::CommitMsg { file } => commit_msg(&file),
    };
    result.unwrap_or_else(|e| {
        eprintln!("Error: {e:#}");
        ExitCode::FAILURE
    })
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

    #[test]
    fn report_fails_on_warnings_only_when_denied() {
        let mut report = Report::new(PathBuf::new(), Vec::new());
        report.should("readme-exists", "README.md", true);
        assert!(!report_fails(&report, false));
        assert!(!report_fails(&report, true));

        report.should("changelog-exists", "CHANGELOG.md", false);
        assert!(!report_fails(&report, false));
        assert!(report_fails(&report, true));

        report.must("license-exists", "LICENSE", false);
        assert!(report_fails(&report, false));
        assert!(report_fails(&report, true));
    }

    #[test]
    fn commit_msg_problems_name_the_file_and_line() {
        let message = format!("feat: {}\n\n{}\n", "a".repeat(70), "b".repeat(73));
        assert_eq!(
            commit_msg_problems(Path::new(".git/COMMIT_EDITMSG"), &message, "#", true),
            [
                ".git/COMMIT_EDITMSG:1: subject is 76 characters long, over 72",
                ".git/COMMIT_EDITMSG:3: body line is 73 characters long, over 72",
            ]
        );
    }
}

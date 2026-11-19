// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Command-line interface.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::checks;
use crate::commit_msg::{self, MAX_LINE_LENGTH};
use crate::context::ProjectContext;
use crate::git;
use crate::models::Language;
use crate::report::{CheckResult, Report};

#[derive(Parser)]
#[command(
    name = "norms",
    version = env!("BUILD_VERSION"),
    about = "Verify and enforce project coding standards."
)]
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

    /// Check a commit message's line lengths and type case, for a commit-msg hook.
    CommitMsg {
        /// Path to the file holding the commit message.
        file: PathBuf,

        /// Whether lines over 72 characters fail or only warn, as in CI, which checks a commit already made.
        #[arg(long, value_enum, env = "NORMS_COMMIT_MSG_LENGTH", default_value_t = LongLines::Fail)]
        long_lines: LongLines,
    },
}

/// What `norms commit-msg` does about lines that are too long.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LongLines {
    /// Fail, as a type not in lowercase does.
    Fail,
    /// Report them, but pass.
    Warn,
}

/// JSON output structure.
#[derive(Serialize)]
struct JsonOutput<'a> {
    path: String,
    languages: BTreeSet<Language>,
    components: Vec<JsonComponent>,
    summary: JsonSummary,
    results: &'a [CheckResult],
}

#[derive(Serialize)]
struct JsonComponent {
    language: Language,
    path: String,
}

#[derive(Serialize)]
struct JsonSummary {
    passed: usize,
    failed: usize,
    warnings: usize,
    skipped: usize,
}

fn json_output(report: &Report) -> anyhow::Result<String> {
    let output = JsonOutput {
        path: report
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| report.project_path.clone())
            .display()
            .to_string(),
        languages: report.languages(),
        components: report
            .components
            .iter()
            .map(|component| JsonComponent {
                language: component.language,
                path: component.display_path(),
            })
            .collect(),
        summary: JsonSummary {
            passed: report.passed(),
            failed: report.failed(),
            warnings: report.warnings(),
            skipped: report.skipped(),
        },
        results: &report.results,
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

/// Describes a commit message's type if it isn't in lowercase, as `<file>:<line>: <problem>`. See
/// [`commit_msg::miscased_type`] for `comment` and `edited`.
fn miscased_type_problem(
    file: &Path,
    message: &str,
    comment: &str,
    edited: bool,
) -> Option<String> {
    commit_msg::miscased_type(message, comment, edited).map(|found| {
        format!(
            "{}:{}: type {} is not in lowercase; write {}",
            file.display(),
            found.number,
            found.found,
            found.found.to_lowercase()
        )
    })
}

/// Describes each line of a commit message that is too long, as `<file>:<line>: <problem>`. See
/// [`commit_msg::long_lines`] for `comment` and `edited`.
fn long_line_problems(file: &Path, message: &str, comment: &str, edited: bool) -> Vec<String> {
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

fn commit_msg(file: &Path, long_lines: LongLines) -> anyhow::Result<ExitCode> {
    let fail_long = long_lines == LongLines::Fail;
    let message = fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
    let message = String::from_utf8_lossy(&message);
    // git runs a commit-msg hook with `GIT_EDITOR=:` when it opens no editor, as for `git commit -m`, and then keeps
    // comment lines.
    let edited = env::var_os("GIT_EDITOR").is_none_or(|editor| editor != ":");
    let comment = git::comment_string();
    let miscased = miscased_type_problem(file, &message, &comment, edited);
    let long = long_line_problems(file, &message, &comment, edited);

    for problem in miscased.iter().chain(&long) {
        eprintln!("{problem}");
    }
    if miscased.is_none() && long.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "Write the type in lowercase, keep the subject within {MAX_LINE_LENGTH} characters (aiming for 50) and wrap \
         the body at {MAX_LINE_LENGTH}; body lines with URLs are exempt."
    );
    if !fail_long && !long.is_empty() {
        eprintln!(
            "The long lines are only a warning, with --long-lines warn (or NORMS_COMMIT_MSG_LENGTH=warn)."
        );
    }
    if miscased.is_some() || fail_long && !long.is_empty() {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// Entry point for the CLI.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Verify {
            path,
            output_json,
            deny_warnings,
        } => verify(path, output_json, deny_warnings),
        Commands::CommitMsg { file, long_lines } => commit_msg(&file, long_lines),
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
    fn json_results_have_exactly_their_fields() {
        let frontend = Component {
            language: Language::TypeScript,
            path: PathBuf::from("frontend"),
        };
        let mut report = Report::new(PathBuf::new(), vec![frontend.clone()]);
        report.must("readme-exists", "README.md", true);
        report.with_component(&frontend, |report| {
            report.should(
                "package-json-private",
                "package.json: private",
                Err("Not private".to_owned()),
            );
        });

        let json: serde_json::Value = serde_json::from_str(&json_output(&report).unwrap()).unwrap();
        assert_eq!(
            json.get("results").unwrap(),
            &serde_json::json!([
                { "id": "readme-exists", "name": "README.md", "status": "pass" },
                {
                    "id": "package-json-private",
                    "name": "package.json: private",
                    "component": "frontend",
                    "status": "warn",
                    "message": "Not private",
                },
            ])
        );
    }

    #[test]
    fn json_names_languages_in_lowercase() {
        let languages = [
            Language::Cpp,
            Language::Python,
            Language::Rust,
            Language::TypeScript,
        ];
        assert_eq!(
            serde_json::to_value(languages).unwrap(),
            serde_json::json!(["cpp", "python", "rust", "typescript"])
        );
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
            report.should(
                "package-json-esm",
                "package.json: uses ESM (type: module)",
                true,
            );
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
        let message = format!("Feat: {}\n\n{}\n", "a".repeat(70), "b".repeat(73));
        let file = Path::new(".git/COMMIT_EDITMSG");
        assert_eq!(
            miscased_type_problem(file, &message, "#", true).as_deref(),
            Some(".git/COMMIT_EDITMSG:1: type Feat is not in lowercase; write feat")
        );
        assert_eq!(
            long_line_problems(file, &message, "#", true),
            [
                ".git/COMMIT_EDITMSG:1: subject is 76 characters long, over 72",
                ".git/COMMIT_EDITMSG:3: body line is 73 characters long, over 72",
            ]
        );
    }

    #[test]
    fn long_lines_take_fail_or_warn() {
        let long_lines = |args: &[&str]| match Cli::try_parse_from(
            [["norms", "commit-msg", "MSG"].as_slice(), args].concat(),
        )?
        .command
        {
            Commands::CommitMsg { long_lines, .. } => Ok::<_, clap::Error>(long_lines),
            Commands::Verify { .. } => unreachable!(),
        };
        assert!(matches!(
            long_lines(&["--long-lines", "warn"]),
            Ok(LongLines::Warn)
        ));
        assert!(matches!(
            long_lines(&["--long-lines", "fail"]),
            Ok(LongLines::Fail)
        ));
        assert!(long_lines(&["--long-lines", "off"]).is_err());
    }

    #[test]
    fn version_option_prints_the_build_version() {
        let error = Cli::try_parse_from(["norms", "--version"]).err().unwrap();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(
            error.to_string(),
            format!("norms {}\n", env!("BUILD_VERSION"))
        );
    }
}

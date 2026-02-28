// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Verification report with ergonomic check recording and colored terminal output.
//!
//! # Design
//!
//! The report API is designed so that each check is recorded with a single call,
//! eliminating the need to repeat check names across pass/fail branches:
//!
//! ```ignore
//! // Severity is explicit: must = FAIL on issue, should = WARN on issue
//! report.must("README.md", file_exists(&path.join("README.md")));
//! report.should("CHANGELOG.md", file_exists(&path.join("CHANGELOG.md")));
//!
//! // For config files, require_parsed handles exists+valid in one call:
//! let Some(config) = report.require_parsed("pyproject.toml", ctx.pyproject()) else { return };
//! report.must("pyproject.toml: has [project]", config.project.is_some());
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::Serialize;

use crate::context::ConfigStatus;
use crate::models::{Language, Status};

//
// Outcome: the result of evaluating a single check
//

/// The outcome of a single check evaluation.
///
/// Checks produce an `Outcome` indicating success or failure with a message.
/// Various `From` implementations allow ergonomic conversion from common types.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// The check passed.
    Pass,
    /// The check failed with an explanation.
    Issue(String),
}

impl Outcome {
    /// Create a failing outcome with the given message.
    pub fn fail(msg: impl Into<String>) -> Self {
        Self::Issue(msg.into())
    }

    /// Returns `true` if this outcome is a pass.
    #[expect(dead_code)] // Placeholder for future use in conditional logic
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// `true` maps to `Pass`, `false` maps to `Issue` with an empty message.
impl From<bool> for Outcome {
    fn from(value: bool) -> Self {
        if value {
            Self::Pass
        } else {
            Self::Issue(String::new())
        }
    }
}

/// `Ok(())` maps to `Pass`, `Err(msg)` maps to `Issue(msg)`.
impl From<Result<(), String>> for Outcome {
    fn from(result: Result<(), String>) -> Self {
        match result {
            Ok(()) => Self::Pass,
            Err(msg) => Self::Issue(msg),
        }
    }
}

//
// Common outcome helpers
//

/// Check whether a file (or directory) exists at the given path.
pub fn file_exists(path: &Path) -> Outcome {
    if path.exists() {
        Outcome::Pass
    } else {
        Outcome::fail(format!(
            "Not found: {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }
}

/// Check whether a string contains a given pattern.
#[expect(dead_code)] // Placeholder for future use in content checks
pub fn contains(haystack: &str, needle: &str) -> Outcome {
    if haystack.contains(needle) {
        Outcome::Pass
    } else {
        Outcome::fail(format!("Missing: {needle}"))
    }
}

/// Check whether a regex pattern matches anywhere in the string.
pub fn matches_regex(haystack: &str, pattern: &str) -> Outcome {
    match regex::Regex::new(pattern) {
        Ok(re) if re.is_match(haystack) => Outcome::Pass,
        Ok(_) => Outcome::fail(format!("Pattern not found: {pattern}")),
        Err(e) => Outcome::fail(format!("Invalid regex: {e}")),
    }
}

//
// CheckResult: a single recorded check
//

/// A single recorded check result.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
}

//
// Report: accumulates check results
//

/// Accumulates check results for a project verification run.
///
/// Provides ergonomic methods for recording checks at different severity levels,
/// as well as helpers for common patterns like requiring a parsed config file.
pub struct Report {
    pub project_path: PathBuf,
    pub languages: HashSet<Language>,
    pub results: Vec<CheckResult>,
}

impl Report {
    /// Create a new empty report for the given project.
    pub fn new(project_path: PathBuf, languages: HashSet<Language>) -> Self {
        Self {
            project_path,
            languages,
            results: Vec::new(),
        }
    }

    /// Record a required check. If the outcome is an issue, it is recorded as `FAIL`.
    pub fn must(&mut self, name: impl Into<String>, outcome: impl Into<Outcome>) -> &mut Self {
        self.record(name.into(), outcome.into(), Status::Fail)
    }

    /// Record a recommended check. If the outcome is an issue, it is recorded as `WARN`.
    pub fn should(&mut self, name: impl Into<String>, outcome: impl Into<Outcome>) -> &mut Self {
        self.record(name.into(), outcome.into(), Status::Warn)
    }

    /// Record a skipped check with a reason.
    #[expect(dead_code)] // Placeholder for future use in conditional logic
    pub fn skip(&mut self, name: impl Into<String>, reason: impl Into<String>) -> &mut Self {
        self.results.push(CheckResult {
            name: name.into(),
            status: Status::Skip,
            message: reason.into(),
        });

        self
    }

    /// Low-level recording: maps an `Outcome` to the appropriate status.
    fn record(&mut self, name: String, outcome: Outcome, fail_status: Status) -> &mut Self {
        let (status, message) = match outcome {
            Outcome::Pass => (Status::Pass, String::new()),
            Outcome::Issue(msg) => (fail_status, msg),
        };

        self.results.push(CheckResult {
            name,
            status,
            message,
        });

        self
    }

    /// Require a parsed config file, recording existence and validity checks.
    ///
    /// This is the primary mechanism for dependency injection between checks.
    /// A "provider" check calls [`ProjectContext`] to get the parse status, then
    /// passes it here. Downstream checks simply use the returned reference.
    ///
    /// Records up to two checks:
    /// - `"{label}: exists"` - always recorded
    /// - `"{label}: valid"` - recorded if the file exists
    ///
    /// Returns `Some(&T)` only when the file exists and parsed successfully.
    pub fn require_parsed<'a, T>(
        &mut self,
        label: &str,
        status: &'a ConfigStatus<T>,
    ) -> Option<&'a T> {
        match status {
            ConfigStatus::NotFound => {
                self.must(format!("{label}: exists"), Outcome::fail("File not found"));
                None
            }
            ConfigStatus::ParseError { error, .. } => {
                self.must(format!("{label}: exists"), Outcome::Pass);
                self.must(format!("{label}: valid"), Outcome::fail(error.clone()));
                None
            }
            ConfigStatus::Ok(value) => {
                self.must(format!("{label}: exists"), Outcome::Pass);
                self.must(format!("{label}: valid"), Outcome::Pass);
                Some(value)
            }
        }
    }

    /// Like [`require_parsed`](Self::require_parsed) but issues a `WARN` instead
    /// of `FAIL` if the file is missing. Useful for optional config files.
    #[expect(dead_code)] // Placeholder for future use in checksq
    pub fn recommend_parsed<'a, T>(
        &mut self,
        label: &str,
        status: &'a ConfigStatus<T>,
    ) -> Option<&'a T> {
        match status {
            ConfigStatus::NotFound => {
                self.should(format!("{label}: exists"), Outcome::fail("File not found"));
                None
            }
            ConfigStatus::ParseError { error, .. } => {
                self.should(format!("{label}: exists"), Outcome::Pass);
                self.must(format!("{label}: valid"), Outcome::fail(error.clone()));
                None
            }
            ConfigStatus::Ok(value) => {
                self.should(format!("{label}: exists"), Outcome::Pass);
                self.must(format!("{label}: valid"), Outcome::Pass);
                Some(value)
            }
        }
    }

    /// Count of checks with status `Pass`.
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Pass)
            .count()
    }

    /// Count of checks with status `Fail`.
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Fail)
            .count()
    }

    /// Count of checks with status `Warn`.
    pub fn warnings(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Warn)
            .count()
    }

    /// Print the report to stdout with colored output.
    pub fn print(&self) {
        let separator = "═".repeat(68);

        println!("{}", separator.bold());
        println!("{}", "  Project Standards Verification Report".bold());
        println!("{}", separator.bold());
        println!("  Path:      {}", self.project_path.display());

        let lang_str = if self.languages.is_empty() {
            "None".dimmed().to_string()
        } else {
            let mut langs: Vec<_> = self
                .languages
                .iter()
                .map(std::string::ToString::to_string)
                .collect();
            langs.sort();
            langs.join(", ")
        };

        println!("  Languages: {lang_str}");
        println!("{}", separator.bold());
        println!();

        // Group by status, showing failures first
        for &status in &[Status::Fail, Status::Warn, Status::Pass, Status::Skip] {
            let items: Vec<_> = self.results.iter().filter(|r| r.status == status).collect();

            if items.is_empty() {
                continue;
            }

            let heading = match status {
                Status::Fail => "Failed".red().bold().to_string(),
                Status::Warn => "Warnings".yellow().bold().to_string(),
                Status::Pass => "Passed".green().bold().to_string(),
                Status::Skip => "Skipped".dimmed().bold().to_string(),
            };
            println!("  {heading}:");

            for item in &items {
                let badge = match item.status {
                    Status::Fail => "FAIL".red().bold(),
                    Status::Warn => "WARN".yellow().bold(),
                    Status::Pass => "PASS".green().bold(),
                    Status::Skip => "SKIP".dimmed().bold(),
                };
                let msg = if item.message.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", item.message.dimmed())
                };
                println!("    [{badge}] {}{msg}", item.name);
            }

            println!();
        }

        // Summary line
        println!("{}", separator.bold());
        let summary = format!(
            "  Summary: {} passed, {} failed, {} warnings",
            self.passed(),
            self.failed(),
            self.warnings()
        );
        if self.failed() > 0 {
            println!("{}", summary.red().bold());
        } else if self.warnings() > 0 {
            println!("{}", summary.yellow().bold());
        } else {
            println!("{}", summary.green().bold());
        }
        println!("{}", separator.bold());
    }
}

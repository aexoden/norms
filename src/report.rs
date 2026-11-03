// SPDX-License-Identifier: MIT OR Apache-2.0
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
//! report.must("readme-exists", "README.md", file_exists(&path.join("README.md")));
//! report.should("changelog-exists", "CHANGELOG.md", file_exists(&path.join("CHANGELOG.md")));
//!
//! // For config files, require_parsed handles exists+valid in one call:
//! let Some(config) = report.require_parsed("pyproject", "pyproject.toml", ctx.pyproject()) else { return };
//! report.must("pyproject-project", "pyproject.toml: has [project]", config.project.is_some());
//! ```
//!
//! # Check IDs
//!
//! Every check has a stable ID, which waivers in `norms.toml` name. IDs are
//! kebab-case: the subject (a file, tool or language) followed by the property
//! checked, such as `cargo-toml-license` or `rust-ci-clippy`. A file's subject
//! is its name without the extension (`tsconfig`, `renovate`), unless the stem
//! alone wouldn't identify the file (`cargo-toml`, `package-json`, `deny-toml`).
//! The name beside the ID is a human-readable description that may change
//! freely; the ID may not, once projects can depend on it.
//!
//! An ID labels one check, which may record several results: one for each
//! file it inspects, such as each referenced tsconfig. All of them share the ID.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use colored::Colorize as _;
use serde::Serialize;

use crate::config::Waiver;
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
pub(crate) enum Outcome {
    /// The check passed.
    Pass,
    /// The check failed with an explanation.
    Issue(String),
}

impl Outcome {
    /// Create a failing outcome with the given message.
    pub(crate) fn fail(msg: impl Into<String>) -> Self {
        Self::Issue(msg.into())
    }

    /// Returns `true` if this outcome is a pass.
    #[expect(dead_code, reason = "placeholder for future use in conditional logic")]
    pub(crate) const fn is_pass(&self) -> bool {
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
pub(crate) fn file_exists(path: &Path) -> Outcome {
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
#[expect(dead_code, reason = "placeholder for future use in content checks")]
pub(crate) fn contains(haystack: &str, needle: &str) -> Outcome {
    if haystack.contains(needle) {
        Outcome::Pass
    } else {
        Outcome::fail(format!("Missing: {needle}"))
    }
}

/// Check whether a regex pattern matches anywhere in the string.
pub(crate) fn matches_regex(haystack: &str, pattern: &str) -> Outcome {
    match regex::Regex::new(pattern) {
        Ok(re) if re.is_match(haystack) => Outcome::Pass,
        Ok(_) => Outcome::fail(format!("Pattern not found: {pattern}")),
        Err(e) => Outcome::fail(format!("Invalid regex: {e}")),
    }
}

/// Whether `id` is a well-formed check ID: lowercase ASCII words of letters
/// and digits, joined by single hyphens.
pub(crate) fn is_valid_check_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

//
// CheckResult: a single recorded check
//

/// A single recorded check result.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CheckResult {
    pub id: String,
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
pub(crate) struct Report {
    pub project_path: PathBuf,
    pub languages: BTreeSet<Language>,
    pub results: Vec<CheckResult>,
}

impl Report {
    /// Create a new empty report for the given project.
    pub(crate) const fn new(project_path: PathBuf, languages: BTreeSet<Language>) -> Self {
        Self {
            project_path,
            languages,
            results: Vec::new(),
        }
    }

    /// Record a required check. If the outcome is an issue, it is recorded as `FAIL`.
    pub(crate) fn must(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        outcome: impl Into<Outcome>,
    ) -> &mut Self {
        self.record(id.into(), name.into(), outcome.into(), Status::Fail)
    }

    /// Record a recommended check. If the outcome is an issue, it is recorded as `WARN`.
    pub(crate) fn should(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        outcome: impl Into<Outcome>,
    ) -> &mut Self {
        self.record(id.into(), name.into(), outcome.into(), Status::Warn)
    }

    /// Low-level recording: maps an `Outcome` to the appropriate status.
    fn record(
        &mut self,
        id: String,
        name: String,
        outcome: Outcome,
        fail_status: Status,
    ) -> &mut Self {
        debug_assert!(is_valid_check_id(&id), "invalid check ID: {id}");

        let (status, message) = match outcome {
            Outcome::Pass => (Status::Pass, String::new()),
            Outcome::Issue(msg) => (fail_status, msg),
        };

        self.results.push(CheckResult {
            id,
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
    /// Records up to two checks, with IDs derived from `id`:
    /// - `{id}-exists`, `"{label}: exists"` - always recorded
    /// - `{id}-valid`, `"{label}: valid"` - recorded if the file exists
    ///
    /// Returns `Some(&T)` only when the file exists and parsed successfully.
    pub(crate) fn require_parsed<'a, T>(
        &mut self,
        id: &str,
        label: &str,
        status: &'a ConfigStatus<T>,
    ) -> Option<&'a T> {
        let (exists, valid) = (format!("{id}-exists"), format!("{id}-valid"));
        let (exists_name, valid_name) = (format!("{label}: exists"), format!("{label}: valid"));

        match status {
            ConfigStatus::NotFound => {
                self.must(exists, exists_name, Outcome::fail("File not found"));
                None
            }
            ConfigStatus::ParseError { error, .. } => {
                self.must(exists, exists_name, Outcome::Pass);
                self.must(valid, valid_name, Outcome::fail(error.clone()));
                None
            }
            ConfigStatus::Ok(value) => {
                self.must(exists, exists_name, Outcome::Pass);
                self.must(valid, valid_name, Outcome::Pass);
                Some(value)
            }
        }
    }

    /// Like [`require_parsed`](Self::require_parsed) but issues a `WARN` instead
    /// of `FAIL` if the file is missing. Useful for optional config files.
    pub(crate) fn recommend_parsed<'a, T>(
        &mut self,
        id: &str,
        label: &str,
        status: &'a ConfigStatus<T>,
    ) -> Option<&'a T> {
        let (exists, valid) = (format!("{id}-exists"), format!("{id}-valid"));
        let (exists_name, valid_name) = (format!("{label}: exists"), format!("{label}: valid"));

        match status {
            ConfigStatus::NotFound => {
                self.should(exists, exists_name, Outcome::fail("File not found"));
                None
            }
            ConfigStatus::ParseError { error, .. } => {
                self.should(exists, exists_name, Outcome::Pass);
                self.must(valid, valid_name, Outcome::fail(error.clone()));
                None
            }
            ConfigStatus::Ok(value) => {
                self.should(exists, exists_name, Outcome::Pass);
                self.must(valid, valid_name, Outcome::Pass);
                Some(value)
            }
        }
    }

    /// Apply the project's waivers. Call this once every other check has been recorded.
    ///
    /// Each failed or warned check that a waiver names becomes `SKIP`, with the waiver's reason as its message. Each
    /// waiver also records a check of its own, which warns if the waiver matched no check or only passing ones, so
    /// that waivers outliving their need, or naming a renamed check, don't linger unnoticed. The checks of `norms.toml`
    /// itself can't be waived, since waiving them would hide problems with the waivers; such a waiver fails.
    pub(crate) fn apply_waivers(&mut self, waivers: &[Waiver]) {
        let mut waived = HashSet::new();

        for waiver in waivers {
            let name = format!("norms.toml: waiver for {} is needed", waiver.check);

            if waiver.check.starts_with("norms-toml-") {
                self.must(
                    "norms-toml-waiver-needed",
                    name,
                    Outcome::fail("norms.toml's own checks can't be waived"),
                );
                continue;
            }

            let mut matched = false;
            let mut needed = false;

            for result in self.results.iter_mut().filter(|r| r.id == waiver.check) {
                matched = true;

                match result.status {
                    Status::Fail | Status::Warn => {
                        result.status = Status::Skip;
                        result.message = format!("Waived: {}", waiver.reason);
                        needed = true;
                    }
                    // Waived by an earlier, duplicate waiver, rather than skipped by the check itself.
                    Status::Skip => needed |= waived.contains(waiver.check.as_str()),
                    Status::Pass => {}
                }
            }

            waived.insert(waiver.check.as_str());

            let outcome = if needed {
                Outcome::Pass
            } else if matched {
                Outcome::fail("The check passes without it")
            } else {
                Outcome::fail("No check with this ID ran")
            };

            self.should("norms-toml-waiver-needed", name, outcome);
        }
    }

    /// Count of checks with status `Pass`.
    pub(crate) fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Pass)
            .count()
    }

    /// Count of checks with status `Fail`.
    pub(crate) fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Fail)
            .count()
    }

    /// Count of checks with status `Warn`.
    pub(crate) fn warnings(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Warn)
            .count()
    }

    /// Count of checks with status `Skip`.
    pub(crate) fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Skip)
            .count()
    }

    /// Print the report to stdout with colored output.
    pub(crate) fn print(&self) {
        let separator = "═".repeat(68);

        println!("{}", separator.bold());
        println!("{}", "  Project Standards Verification Report".bold());
        println!("{}", separator.bold());
        println!("  Path:      {}", self.project_path.display());

        let lang_str = if self.languages.is_empty() {
            "None".dimmed().to_string()
        } else {
            let langs: Vec<_> = self.languages.iter().map(ToString::to_string).collect();
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
                println!("    [{badge}] {}: {}{msg}", item.id, item.name);
            }

            println!();
        }

        // Summary line
        println!("{}", separator.bold());
        let summary = format!(
            "  Summary: {} passed, {} failed, {} warnings, {} skipped",
            self.passed(),
            self.failed(),
            self.warnings(),
            self.skipped()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ids_are_kebab_case() {
        for id in [
            "readme-exists",
            "cargo-toml-license",
            "rust-ci-clippy",
            "a1-b2",
        ] {
            assert!(is_valid_check_id(id), "{id} should be valid");
        }

        for id in [
            "",
            "-readme",
            "readme-",
            "readme--exists",
            "Readme",
            "readme_exists",
        ] {
            assert!(!is_valid_check_id(id), "{id} should be invalid");
        }
    }

    #[test]
    fn require_parsed_derives_ids_from_prefix() {
        let mut report = Report::new(PathBuf::new(), BTreeSet::new());
        let status = ConfigStatus::<()>::ParseError {
            raw: String::new(),
            error: "bad".into(),
        };

        assert!(
            report
                .require_parsed("cargo-toml", "Cargo.toml", &status)
                .is_none()
        );

        let ids: Vec<_> = report.results.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["cargo-toml-exists", "cargo-toml-valid"]);
        assert_eq!(report.failed(), 1);
    }

    fn waiver(check: &str) -> Waiver {
        Waiver {
            check: check.into(),
            reason: "Because.".into(),
        }
    }

    fn statuses(report: &Report) -> Vec<(&str, Status, &str)> {
        report
            .results
            .iter()
            .map(|r| (r.id.as_str(), r.status, r.message.as_str()))
            .collect()
    }

    #[test]
    fn waivers_skip_failures_and_warnings() {
        let mut report = Report::new(PathBuf::new(), BTreeSet::new());
        report.must("readme-exists", "README.md", false);
        report.should("changelog-exists", "CHANGELOG.md", false);
        report.apply_waivers(&[waiver("readme-exists"), waiver("changelog-exists")]);

        assert_eq!(
            statuses(&report),
            [
                ("readme-exists", Status::Skip, "Waived: Because."),
                ("changelog-exists", Status::Skip, "Waived: Because."),
                ("norms-toml-waiver-needed", Status::Pass, ""),
                ("norms-toml-waiver-needed", Status::Pass, ""),
            ]
        );
        assert_eq!(
            (report.failed(), report.warnings(), report.skipped()),
            (0, 0, 2)
        );
    }

    #[test]
    fn waivers_for_passing_or_absent_checks_warn() {
        let mut report = Report::new(PathBuf::new(), BTreeSet::new());
        report.must("readme-exists", "README.md", true);
        report.apply_waivers(&[waiver("readme-exists"), waiver("no-such-check")]);

        assert_eq!(
            statuses(&report),
            [
                ("readme-exists", Status::Pass, ""),
                (
                    "norms-toml-waiver-needed",
                    Status::Warn,
                    "The check passes without it"
                ),
                (
                    "norms-toml-waiver-needed",
                    Status::Warn,
                    "No check with this ID ran"
                ),
            ]
        );
    }

    #[test]
    fn duplicate_waivers_both_count_as_needed() {
        let mut report = Report::new(PathBuf::new(), BTreeSet::new());
        report.must("readme-exists", "README.md", false);
        report.apply_waivers(&[waiver("readme-exists"), waiver("readme-exists")]);

        assert_eq!(report.warnings(), 0);
        assert_eq!(report.skipped(), 1);
    }

    #[test]
    fn norms_toml_checks_cannot_be_waived() {
        let mut report = Report::new(PathBuf::new(), BTreeSet::new());
        report.must(
            "norms-toml-waiver-reasons",
            "norms.toml: every waiver has a reason",
            false,
        );
        report.apply_waivers(&[
            waiver("no-such-check"),
            waiver("norms-toml-waiver-reasons"),
            waiver("norms-toml-waiver-needed"),
        ]);

        assert_eq!(
            statuses(&report),
            [
                ("norms-toml-waiver-reasons", Status::Fail, ""),
                (
                    "norms-toml-waiver-needed",
                    Status::Warn,
                    "No check with this ID ran"
                ),
                (
                    "norms-toml-waiver-needed",
                    Status::Fail,
                    "norms.toml's own checks can't be waived"
                ),
                (
                    "norms-toml-waiver-needed",
                    Status::Fail,
                    "norms.toml's own checks can't be waived"
                ),
            ]
        );
    }
}

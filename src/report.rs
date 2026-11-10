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
//! report.should("mypy-strict", "mypy: strict mode", mypy.strict == Some(true));
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
//!
//! # Severity
//!
//! A check records a failure with `must` and a warning with `should`, or, when
//! its issues differ in severity, each [`Issue`] at its own with `check`.
//! Choose by what the issue means, as STANDARDS.md's "norms Configuration"
//! section states: a failure when a safeguard the standards require is absent
//! or unusable (a required file is missing, a config can't be parsed, a tool
//! isn't configured or run at all, or it is set up so that it can't run or
//! passes what it should fail), and a warning when it's present but differs
//! from the standard (weaker settings, other flags, versions, pins, names or
//! metadata), when norms can't be sure of the issue, or when config is merely
//! stale, such as an unneeded waiver. What a tool already catches by failing a
//! hook or CI job with an error that names the problem, such as a configuration
//! that loads a package missing from the dependencies, isn't checked. A
//! malformed `norms.toml` fails. Three safeguards are an exception to warning
//! when norms can't be sure: the gate job's failing step, and the
//! setup-rust-toolchain `toolchain` and setup-uv `python-version` inputs of the
//! jobs testing a library's oldest supported Rust and Python. norms can't tell
//! one written another way from one that doesn't work, so it fails as absent in
//! any other form.
//!
//! `norms verify` exits with failure if any check fails, or with
//! `--deny-warnings` if any check fails or warns.
//!
//! # Components
//!
//! A language's checks of its own files run once for each component of that
//! language, inside [`Report::with_component`], so each result they record
//! names its component. Checks of files that belong to the whole repository,
//! such as `.pre-commit-config.yaml`, record no component.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use colored::Colorize as _;
use serde::Serialize;

use crate::config::Waiver;
use crate::context::ConfigStatus;
use crate::models::{Component, Language, Status};

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
}

/// How serious an [`Issue`] is: whether its check fails or warns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Severity {
    Warn,
    Fail,
}

/// An issue a check found, with its own severity, for a check whose issues differ in severity: such as a hook that
/// fails when it is missing but warns when it runs in another stage. [`Report::check`] records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Issue {
    pub severity: Severity,
    pub message: String,
}

impl Issue {
    /// An issue that fails its check.
    pub(crate) fn fail(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Fail,
            message: message.into(),
        }
    }

    /// An issue that warns.
    pub(crate) fn warn(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            message: message.into(),
        }
    }

    /// The issues of requirements that must all hold, if there are any: their messages joined with `"; "`, and the
    /// most severe of their severities.
    pub(crate) fn combine(issues: impl IntoIterator<Item = Self>) -> Result<(), Self> {
        Self::join(issues, Severity::max)
    }

    /// The issues of alternatives, such as several hooks with the same ID, any of which would do, if there are any:
    /// their messages joined with `"; "`, and the least severe of their severities, since the best alternative
    /// decides.
    pub(crate) fn alternatives(issues: impl IntoIterator<Item = Self>) -> Result<(), Self> {
        Self::join(issues, Severity::min)
    }

    fn join(
        issues: impl IntoIterator<Item = Self>,
        severity: impl Fn(Severity, Severity) -> Severity,
    ) -> Result<(), Self> {
        let mut joined: Option<Self> = None;
        for issue in issues {
            match &mut joined {
                None => joined = Some(issue),
                Some(joined) => {
                    joined.severity = severity(joined.severity, issue.severity);
                    joined.message.push_str("; ");
                    joined.message.push_str(&issue.message);
                }
            }
        }
        joined.map_or(Ok(()), Err)
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
///
/// It serializes as an entry of `norms verify --json`'s `results`, so its fields and their serde attributes are that
/// output's format: a field added here appears there.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CheckResult {
    pub id: String,
    pub name: String,
    /// The display path of the component the check inspected, if it inspected one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
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
    pub components: Vec<Component>,
    pub results: Vec<CheckResult>,
}

impl Report {
    /// Create a new empty report for the given project and its components.
    pub(crate) const fn new(project_path: PathBuf, components: Vec<Component>) -> Self {
        Self {
            project_path,
            components,
            results: Vec::new(),
        }
    }

    /// The languages of the project's components, in a stable order.
    pub(crate) fn languages(&self) -> BTreeSet<Language> {
        self.components
            .iter()
            .map(|component| component.language)
            .collect()
    }

    /// Run `checks`, recording each of their results as belonging to `component`.
    pub(crate) fn with_component(&mut self, component: &Component, checks: impl FnOnce(&mut Self)) {
        let start = self.results.len();
        checks(self);

        let path = component.display_path();
        for result in self.results.iter_mut().skip(start) {
            result.component.get_or_insert_with(|| path.clone());
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

    /// Record a check whose issues each carry their own severity: `FAIL` or `WARN`, as the issue says.
    pub(crate) fn check(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        outcome: Result<(), Issue>,
    ) -> &mut Self {
        let (outcome, status) = match outcome {
            Ok(()) => (Outcome::Pass, Status::Pass),
            Err(issue) => (
                Outcome::Issue(issue.message),
                match issue.severity {
                    Severity::Fail => Status::Fail,
                    Severity::Warn => Status::Warn,
                },
            ),
        };
        self.record(id.into(), name.into(), outcome, status)
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
            component: None,
            status,
            message,
        });

        self
    }

    /// Require a parsed config file, recording existence and validity checks.
    ///
    /// This is the primary mechanism for dependency injection between checks.
    /// A "provider" check calls [`ProjectContext`](crate::context::ProjectContext) to get the parse status, then
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
            ConfigStatus::ParseError { error } => {
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
        println!("  Path:       {}", self.project_path.display());

        let components = if self.components.is_empty() {
            "None".dimmed().to_string()
        } else {
            let components: Vec<_> = self
                .components
                .iter()
                .map(|component| format!("{} ({})", component.language, component.display_path()))
                .collect();
            components.join(", ")
        };

        println!("  Components: {components}");
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
                let component = match &item.component {
                    Some(path) if path != "." => format!("[{path}] "),
                    Some(_) | None => String::new(),
                };
                println!("    [{badge}] {}: {component}{}{msg}", item.id, item.name);
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
        let mut report = Report::new(PathBuf::new(), Vec::new());
        let status = ConfigStatus::<()>::ParseError {
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

    #[test]
    fn with_component_labels_only_its_results() {
        let mut report = Report::new(PathBuf::new(), Vec::new());
        let frontend = Component {
            language: Language::TypeScript,
            path: PathBuf::from("frontend"),
        };

        report.should("readme-exists", "README.md", true);
        report.with_component(&frontend, |report| {
            report.should(
                "package-json-esm",
                "package.json: uses ESM (type: module)",
                true,
            );
        });
        report.should("changelog-exists", "CHANGELOG.md", true);

        let components: Vec<_> = report
            .results
            .iter()
            .map(|r| r.component.as_deref())
            .collect();
        assert_eq!(components, [None, Some("frontend"), None]);
    }

    #[test]
    fn check_records_each_issue_at_its_own_severity() {
        let mut report = Report::new(PathBuf::new(), Vec::new());
        report.check("precommit-reuse", "Pre-commit: reuse hook", Ok(()));
        report.check(
            "precommit-reuse",
            "Pre-commit: reuse hook",
            Err(Issue::fail("No reuse hook")),
        );
        report.check(
            "precommit-reuse",
            "Pre-commit: reuse hook",
            Err(Issue::warn(
                "The reuse hook doesn't run in the pre-commit stage",
            )),
        );

        assert_eq!(
            statuses(&report),
            [
                ("precommit-reuse", Status::Pass, ""),
                ("precommit-reuse", Status::Fail, "No reuse hook"),
                (
                    "precommit-reuse",
                    Status::Warn,
                    "The reuse hook doesn't run in the pre-commit stage"
                ),
            ]
        );
    }

    #[test]
    fn combined_issues_take_the_most_severe() {
        assert_eq!(Issue::combine([]), Ok(()));
        assert_eq!(
            Issue::combine([Issue::warn("a"), Issue::fail("b"), Issue::warn("c")]),
            Err(Issue::fail("a; b; c"))
        );
        assert_eq!(
            Issue::combine([Issue::warn("a"), Issue::warn("b")]),
            Err(Issue::warn("a; b"))
        );
    }

    #[test]
    fn alternatives_take_the_least_severe() {
        assert_eq!(Issue::alternatives([]), Ok(()));
        assert_eq!(
            Issue::alternatives([Issue::fail("a"), Issue::warn("b")]),
            Err(Issue::warn("a; b"))
        );
        assert_eq!(
            Issue::alternatives([Issue::fail("a"), Issue::fail("b")]),
            Err(Issue::fail("a; b"))
        );
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
        let mut report = Report::new(PathBuf::new(), Vec::new());
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
        let mut report = Report::new(PathBuf::new(), Vec::new());
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
        let mut report = Report::new(PathBuf::new(), Vec::new());
        report.must("readme-exists", "README.md", false);
        report.apply_waivers(&[waiver("readme-exists"), waiver("readme-exists")]);

        assert_eq!(report.warnings(), 0);
        assert_eq!(report.skipped(), 1);
    }

    #[test]
    fn norms_toml_checks_cannot_be_waived() {
        let mut report = Report::new(PathBuf::new(), Vec::new());
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

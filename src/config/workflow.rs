// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for a GitHub Actions workflow file.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Top-level workflow structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct Workflow {
    pub jobs: BTreeMap<String, WorkflowJob>,
}

/// A single job in a workflow.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct WorkflowJob {
    pub steps: Vec<WorkflowStep>,
}

/// A single step in a job, which either runs an action (`uses`) or a script (`run`).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct WorkflowStep {
    pub uses: Option<String>,
    pub run: Option<String>,
}

impl Workflow {
    fn steps(&self) -> impl Iterator<Item = &WorkflowStep> {
        self.jobs.values().flat_map(|job| &job.steps)
    }

    /// Whether any step's `run` script contains `command`. Commands that reach a script only through an expression,
    /// such as `${{ matrix.command }}`, are not seen.
    pub(crate) fn runs_command(&self, command: &str) -> bool {
        self.steps()
            .filter_map(|step| step.run.as_deref())
            .any(|run| run.contains(command))
    }

    /// Whether any step uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref.
    pub(crate) fn uses_action(&self, action: &str) -> bool {
        self.steps()
            .filter_map(|step| step.uses.as_deref())
            .any(|uses| is_action(uses, action))
    }
}

/// Whether `uses` refers to `action`, ignoring the ref after `@`. GitHub owner and repository names are
/// case-insensitive, but a path within the repository is not.
fn is_action(uses: &str, action: &str) -> bool {
    let (name, _) = uses.split_once('@').unwrap_or((uses, ""));
    let (repo, path) = split_repo(name);
    let (action_repo, action_path) = split_repo(action);
    repo.eq_ignore_ascii_case(action_repo) && path == action_path
}

/// Split `owner/repo/path` into `owner/repo` and the path, which may be empty.
fn split_repo(name: &str) -> (&str, &str) {
    match name.match_indices('/').nth(1) {
        Some((index, _)) => name.split_at(index),
        None => (name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW: &str = "
# cargo fmt and EmbarkStudios/cargo-deny-action, in a comment
name: CI
on: [push]
jobs:
  call:
    uses: owner/repo/.github/workflows/reusable.yaml@main
  rustfmt:
    name: Run cargo fmt
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Check formatting
        uses: Actions-Rust-Lang/RustFmt@4066006ec54a31931b9b1fddfd38f2fdf2d27143 # v1.1.2
  test:
    steps:
      - name: cargo deny
        run: |
          cargo build
          cargo test --workspace
";

    fn workflow() -> Workflow {
        serde_saphyr::from_str(WORKFLOW).expect("workflow parses")
    }

    #[test]
    fn finds_commands_in_run_scripts() {
        let workflow = workflow();
        assert!(workflow.runs_command("cargo test"));
        assert!(workflow.runs_command("cargo build"));
    }

    #[test]
    fn ignores_commands_outside_run_scripts() {
        let workflow = workflow();
        assert!(!workflow.runs_command("cargo fmt"));
        assert!(!workflow.runs_command("cargo deny"));
        assert!(!workflow.runs_command("actions/checkout"));
    }

    #[test]
    fn finds_actions_at_any_ref() {
        let workflow = workflow();
        assert!(workflow.uses_action("actions/checkout"));
        assert!(workflow.uses_action("actions-rust-lang/rustfmt"));
    }

    #[test]
    fn ignores_other_actions() {
        let workflow = workflow();
        assert!(!workflow.uses_action("actions"));
        assert!(!workflow.uses_action("actions/check"));
        assert!(!workflow.uses_action("actions-rust-lang/rustfmt/sub"));
        assert!(!workflow.uses_action("EmbarkStudios/cargo-deny-action"));
        assert!(!workflow.uses_action("owner/repo"));
        assert!(!workflow.runs_command("cargo clippy"));
    }

    #[test]
    fn matches_paths_within_a_repository() {
        assert!(is_action("Owner/Repo/sub/path@v1", "owner/repo/sub/path"));
        assert!(!is_action("owner/repo/Sub/path@v1", "owner/repo/sub/path"));
        assert!(!is_action("owner/repo/sub@v1", "owner/repo"));
        assert!(!is_action("owner/repo@v1", "owner/repo/sub"));
        assert!(!is_action(
            "./.github/actions/rustfmt",
            "actions-rust-lang/rustfmt"
        ));
    }

    #[test]
    fn parses_an_empty_workflow() {
        let workflow: Workflow = serde_saphyr::from_str("name: CI\n").expect("workflow parses");
        assert!(!workflow.uses_action("actions/checkout"));
        assert!(!workflow.runs_command("cargo test"));
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for a GitHub Actions workflow file.

use std::borrow::Cow;
use std::collections::BTreeMap;

use github_actions_expressions::call::{Call, Function};
use github_actions_expressions::context::Context;
use github_actions_expressions::literal::Literal;
use github_actions_expressions::op::{BinExpr, BinOp};
use github_actions_expressions::{Expr, SpannedExpr};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

/// The ID of `ci.yaml`'s gate job, which succeeds only if every other job did.
pub(crate) const GATE_JOB: &str = "ci-success";

/// The results a needed job can have other than `success`, each of which must fail the gate.
const UNSUCCESSFUL_RESULTS: [&str; 3] = ["failure", "cancelled", "skipped"];

/// Top-level workflow structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct Workflow {
    /// The events that trigger the workflow: one event's name, a list of names, or a mapping of each event to its
    /// configuration.
    pub on: Option<Value>,
    /// A group name, or a mapping with `group` and `cancel-in-progress`.
    pub concurrency: Option<Value>,
    pub jobs: BTreeMap<String, WorkflowJob>,
}

/// A single job in a workflow.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct WorkflowJob {
    /// The condition under which the job runs: an expression, with or without `${{ }}`.
    #[serde(rename = "if")]
    pub condition: Option<Value>,
    /// The ID of the job this one waits for, or a list of them.
    pub needs: Option<Value>,
    pub steps: Vec<WorkflowStep>,
    /// The reusable workflow the job calls instead of running steps.
    pub uses: Option<String>,
    /// A number of minutes, or an expression giving one.
    #[serde(rename = "timeout-minutes")]
    pub timeout_minutes: Option<Value>,
}

/// A single step in a job, which either runs an action (`uses`) or a script (`run`).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct WorkflowStep {
    /// The condition under which the step runs: an expression, with or without `${{ }}`.
    #[serde(rename = "if")]
    pub condition: Option<Value>,
    /// Whether the job carries on, and can succeed, when the step fails: a boolean, or an expression giving one.
    #[serde(rename = "continue-on-error")]
    pub continue_on_error: Option<Value>,
    pub uses: Option<String>,
    pub run: Option<String>,
    /// The action's inputs, kept as YAML wrote them rather than as strings, so that a list or mapping where a string
    /// belongs fails only the check reading it. A value serde can't represent at all, such as `.nan`, still fails the
    /// whole workflow.
    pub with: BTreeMap<String, Value>,
}

impl WorkflowStep {
    /// The value of the action input `name` as the action receives it: a string, whether the YAML wrote it as one or
    /// as a boolean or number. A null or collection is no value.
    pub(crate) fn input(&self, name: &str) -> Option<Cow<'_, str>> {
        match self.with.get(name)? {
            Value::String(value) => Some(Cow::Borrowed(value)),
            Value::Bool(value) => Some(Cow::Owned(value.to_string())),
            Value::Number(value) => Some(Cow::Owned(value.to_string())),
            Value::Null | Value::Array(_) | Value::Object(_) => None,
        }
    }

    /// Whether the step's script exits with an error: one of its commands is `exit` with a status other than 0, or
    /// `false`.
    fn exits_with_error(&self) -> bool {
        self.run.as_deref().is_some_and(|run| {
            join_continued_lines(run).iter().any(|line| {
                commands(line).any(|words| {
                    // A comment ends the command, and a command may follow a keyword such as `then`.
                    let end = words
                        .iter()
                        .position(|word| word.starts_with('#'))
                        .unwrap_or(words.len());
                    let words = words.get(..end).unwrap_or_default();
                    let start = words
                        .iter()
                        .position(|word| !matches!(*word, "then" | "else" | "do" | "{"))
                        .unwrap_or(words.len());
                    match words.get(start..).unwrap_or_default() {
                        ["false"] => true,
                        ["exit", status] => *status != "0",
                        _ => false,
                    }
                })
            })
        })
    }

    /// Whether the step can fail its job: it doesn't set `continue-on-error`, or sets it to `false`. An expression may
    /// give `true`, so it counts as letting the job carry on.
    const fn can_fail_job(&self) -> bool {
        matches!(
            self.continue_on_error,
            None | Some(Value::Null | Value::Bool(false))
        )
    }
}

impl WorkflowJob {
    /// The IDs of the jobs this job needs, or why `needs` isn't a job ID or a list of them.
    pub(crate) fn needed_jobs(&self) -> Result<Vec<&str>, String> {
        match &self.needs {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::String(id)) => Ok(vec![id.as_str()]),
            Some(Value::Array(ids)) => ids
                .iter()
                .map(|id| {
                    id.as_str()
                        .ok_or_else(|| format!("needs lists {id}, which isn't a job ID"))
                })
                .collect(),
            Some(needs) => Err(format!("needs is {needs}, not a job ID or a list of them")),
        }
    }

    /// Whether the job runs whatever the results of the jobs it needs: its `if` is `always()`. Without it, the job is
    /// skipped when one of them fails, and a skipped job satisfies a required check.
    pub(crate) fn runs_always(&self) -> bool {
        parse_condition(self.condition.as_ref()).is_some_and(|condition| {
            matches!(
                condition.inner,
                Expr::Call(Call {
                    func: Function::Always,
                    ..
                })
            )
        })
    }

    /// The results other than `success`, of `failure`, `cancelled` and `skipped`, that the job doesn't fail on when a
    /// job it needs has them. A step fails the job on a result if it exits with an error, and its `if` holds whenever a
    /// needed job has that result: it is `contains(needs.*.result, '<result>')`, alone or among alternatives joined by
    /// `||`.
    pub(crate) fn results_not_failing(&self) -> Vec<&'static str> {
        let failing: Vec<String> = self
            .steps
            .iter()
            .filter(|step| step.can_fail_job() && step.exits_with_error())
            .filter_map(|step| parse_condition(step.condition.as_ref()))
            .flat_map(|condition| needs_results_checked(&condition))
            .collect();
        UNSUCCESSFUL_RESULTS
            .into_iter()
            .filter(|result| {
                !failing
                    .iter()
                    .any(|failing| failing.eq_ignore_ascii_case(result))
            })
            .collect()
    }

    /// Every command line of the job's `run` scripts, with lines continued by a trailing backslash joined into one.
    pub(crate) fn run_lines(&self) -> impl Iterator<Item = String> {
        self.steps
            .iter()
            .filter_map(|step| step.run.as_deref())
            .flat_map(join_continued_lines)
    }

    /// Every step of the job that uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref.
    pub(crate) fn steps_using<'a>(
        &'a self,
        action: &'a str,
    ) -> impl Iterator<Item = &'a WorkflowStep> {
        self.steps.iter().filter(move |step| {
            step.uses
                .as_deref()
                .is_some_and(|uses| is_action(uses, action))
        })
    }
}

impl Workflow {
    fn steps(&self) -> impl Iterator<Item = &WorkflowStep> {
        self.jobs.values().flat_map(|job| &job.steps)
    }

    /// Whether the workflow's `concurrency` cancels superseded runs: a mapping naming a `group` and setting
    /// `cancel-in-progress: true`. A group name alone queues runs instead, and an expression for `cancel-in-progress`
    /// may cancel only some.
    pub(crate) fn cancels_superseded_runs(&self) -> Result<(), String> {
        match &self.concurrency {
            None | Some(Value::Null) => Err("No concurrency group".to_owned()),
            Some(Value::Object(concurrency)) if !has_group(concurrency.get("group")) => {
                Err("concurrency has no group".to_owned())
            }
            Some(Value::Object(concurrency))
                if concurrency.get("cancel-in-progress") == Some(&Value::Bool(true)) =>
            {
                Ok(())
            }
            Some(_) => Err("concurrency doesn't set cancel-in-progress: true".to_owned()),
        }
    }

    /// Whether the workflow runs on a schedule: `on` maps `schedule` to at least one entry. A `schedule` given only by
    /// name, as a string or in a list, sets no times and never runs.
    pub(crate) fn runs_on_schedule(&self) -> bool {
        let Some(Value::Object(on)) = &self.on else {
            return false;
        };
        matches!(on.get("schedule"), Some(Value::Array(entries)) if !entries.is_empty())
    }

    /// The IDs of the jobs that don't set `timeout-minutes`. A job calling a reusable workflow can't set it, so it
    /// isn't counted: the called workflow's own jobs set theirs.
    pub(crate) fn jobs_without_timeout(&self) -> Vec<&str> {
        self.jobs
            .iter()
            .filter(|(_, job)| job.uses.is_none() && job.timeout_minutes.is_none())
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Every step's `run` script.
    pub(crate) fn run_scripts(&self) -> impl Iterator<Item = &str> {
        self.steps().filter_map(|step| step.run.as_deref())
    }

    /// Every command line of every step's `run` script, with lines continued by a trailing backslash joined into one.
    pub(crate) fn run_lines(&self) -> impl Iterator<Item = String> {
        self.run_scripts().flat_map(join_continued_lines)
    }

    /// Every step that uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref.
    pub(crate) fn steps_using<'a>(
        &'a self,
        action: &'a str,
    ) -> impl Iterator<Item = &'a WorkflowStep> {
        self.jobs
            .values()
            .flat_map(move |job| job.steps_using(action))
    }

    /// Every step that uses the action `action`, in each job with a script line for which `runs` holds: for example,
    /// the steps installing the toolchain a job runs a command with.
    pub(crate) fn steps_using_in_jobs_running<'a>(
        &'a self,
        action: &'a str,
        runs: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = &'a WorkflowStep> {
        self.jobs
            .values()
            .filter(move |job| job.run_lines().any(|line| runs(&line)))
            .flat_map(move |job| job.steps_using(action))
    }

    /// Whether any step's `run` script contains `command`. Commands that reach a script only through an expression,
    /// such as `${{ matrix.command }}`, are not seen.
    pub(crate) fn runs_command(&self, command: &str) -> bool {
        self.run_scripts().any(|run| run.contains(command))
    }

    /// Whether any step's `run` script matches `pattern`, for a command that a plain substring would confuse with
    /// another, such as `reuse lint` and `reuse lint-file`.
    pub(crate) fn runs_matching(&self, pattern: &Regex) -> bool {
        self.run_scripts().any(|run| pattern.is_match(run))
    }

    /// Whether any step uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref.
    pub(crate) fn uses_action(&self, action: &str) -> bool {
        self.steps_using(action).next().is_some()
    }
}

/// An `if` condition's expression, with any `${{ }}` around it removed, parsed. A condition that isn't a string, or
/// doesn't parse, is none.
fn parse_condition(condition: Option<&Value>) -> Option<SpannedExpr<'_>> {
    let Some(Value::String(condition)) = condition else {
        return None;
    };
    let condition = condition.trim();
    let condition = condition
        .strip_prefix("${{")
        .and_then(|condition| condition.strip_suffix("}}"))
        .unwrap_or(condition);
    Expr::parse(condition).ok()
}

/// The results that `condition` holds for when a needed job has one: the `<result>` of each
/// `contains(needs.*.result, '<result>')` in it, alone or among alternatives joined by `||`.
fn needs_results_checked(condition: &SpannedExpr<'_>) -> Vec<String> {
    match &condition.inner {
        Expr::BinExpr(BinExpr {
            lhs,
            op: BinOp::Or,
            rhs,
        }) => {
            let mut results = needs_results_checked(lhs);
            results.extend(needs_results_checked(rhs));
            results
        }
        Expr::Call(Call {
            func: Function::Contains,
            args,
        }) => match args.as_slice() {
            [
                SpannedExpr {
                    inner: Expr::Context(context),
                    ..
                },
                SpannedExpr {
                    inner: Expr::Literal(Literal::String(result)),
                    ..
                },
            ] if is_needs_results(context) => {
                vec![result.to_string()]
            }
            _ => Vec::new(),
        },
        Expr::Literal(_)
        | Expr::Star
        | Expr::Call(_)
        | Expr::Identifier(_)
        | Expr::Index(_)
        | Expr::Context(_)
        | Expr::BinExpr(_)
        | Expr::UnExpr { .. } => Vec::new(),
    }
}

/// Whether `context` is exactly `needs.*.result`, the results of all the jobs a job needs. An index such as
/// `needs[0]` or `needs['*']` looks up a single entry of `needs`, which no job ID matches.
fn is_needs_results(context: &Context<'_>) -> bool {
    let identifier = |part: &SpannedExpr<'_>, name: &str| matches!(&part.inner, Expr::Identifier(identifier) if identifier.as_str().eq_ignore_ascii_case(name));
    match context.parts.as_slice() {
        [needs, star, result] => {
            identifier(needs, "needs")
                && matches!(star.inner, Expr::Star)
                && identifier(result, "result")
        }
        _ => false,
    }
}

/// The commands of a script line, split at the shell operators `&&`, `||`, `|` and `;`, each as its words.
pub(crate) fn commands(line: &str) -> impl Iterator<Item = Vec<&str>> {
    line.split(['&', '|', ';'])
        .map(|command| command.split_whitespace().collect())
}

/// Whether a `concurrency` mapping's `group` names one: a non-blank string, or a number.
fn has_group(group: Option<&Value>) -> bool {
    match group {
        Some(Value::String(group)) => !group.trim().is_empty(),
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

/// The lines of a shell script, with each line ending in a backslash joined to the next.
fn join_continued_lines(script: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for line in script.lines() {
        if let Some(head) = line.strip_suffix('\\') {
            current.push_str(head);
            current.push(' ');
        } else {
            current.push_str(line);
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
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
        with:
          flag: false
          count: 3
          text: plain
          quoted: \"false\"
          empty:
          list: [a]
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
    fn matches_commands_by_pattern() {
        let workflow = workflow();
        assert!(workflow.runs_matching(&Regex::new(r"(?m)^\s*cargo test\b").unwrap()));
        assert!(!workflow.runs_matching(&Regex::new(r"\bcargo fmt\b").unwrap()));
    }

    #[test]
    fn finds_steps_of_jobs_running_a_command() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  msrv:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: 1.85
      - run: cargo check
  other:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: nightly
      - run: cargo fmt
",
        )
        .expect("workflow parses");
        let toolchains: Vec<_> = workflow
            .steps_using_in_jobs_running("actions-rust-lang/setup-rust-toolchain", |line| {
                line.contains("cargo check")
            })
            .filter_map(|step| step.input("toolchain"))
            .collect();
        assert_eq!(toolchains, ["1.85"]);
    }

    #[test]
    fn joins_continued_lines() {
        assert_eq!(
            join_continued_lines("one \\\n  two\nthree\nfour \\"),
            ["one    two", "three", "four  "]
        );
        assert_eq!(join_continued_lines(""), Vec::<String>::new());
    }

    #[test]
    fn reads_action_inputs_as_strings() {
        let workflow = workflow();
        let step = workflow
            .steps_using("actions-rust-lang/rustfmt")
            .next()
            .expect("rustfmt step");
        assert_eq!(step.input("flag").as_deref(), Some("false"));
        assert_eq!(step.input("count").as_deref(), Some("3"));
        assert_eq!(step.input("text").as_deref(), Some("plain"));
        assert_eq!(step.input("quoted").as_deref(), Some("false"));
        assert_eq!(step.input("empty"), None);
        assert_eq!(step.input("list"), None);
        assert_eq!(step.input("missing"), None);
    }

    #[test]
    fn finds_jobs_without_timeouts() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  timed:
    timeout-minutes: 10
  expression:
    timeout-minutes: ${{ inputs.timeout }}
  untimed:
    steps: []
  empty:
    timeout-minutes:
  call:
    uses: ./.github/workflows/reusable.yaml
",
        )
        .expect("workflow parses");
        assert_eq!(workflow.jobs_without_timeout(), ["empty", "untimed"]);
    }

    #[test]
    fn concurrency_must_cancel_superseded_runs() {
        let cancels = |yaml: &str| {
            serde_saphyr::from_str::<Workflow>(yaml)
                .expect("workflow parses")
                .cancels_superseded_runs()
        };
        assert_eq!(
            cancels(
                "concurrency:\n  group: ${{ github.workflow }}-${{ github.ref }}\n  cancel-in-progress: true\n"
            ),
            Ok(())
        );

        let no_cancel = Err("concurrency doesn't set cancel-in-progress: true".to_owned());
        for yaml in [
            "concurrency: ci\n",
            "concurrency:\n  group: ci\n",
            "concurrency:\n  group: ci\n  cancel-in-progress: false\n",
            "concurrency:\n  group: ci\n  cancel-in-progress: \"true\"\n",
            "concurrency:\n  group: ci\n  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n",
            "concurrency: [ci]\n",
        ] {
            assert_eq!(cancels(yaml), no_cancel, "{yaml}");
        }
        for yaml in [
            "concurrency:\n  cancel-in-progress: true\n",
            "concurrency:\n  group:\n  cancel-in-progress: true\n",
            "concurrency:\n  group: ' '\n  cancel-in-progress: true\n",
            "concurrency:\n  group: [ci]\n  cancel-in-progress: true\n",
        ] {
            assert_eq!(
                cancels(yaml),
                Err("concurrency has no group".to_owned()),
                "{yaml}"
            );
        }
        for yaml in ["name: CI\n", "concurrency:\n"] {
            assert_eq!(
                cancels(yaml),
                Err("No concurrency group".to_owned()),
                "{yaml}"
            );
        }
    }

    #[test]
    fn finds_schedule_triggers() {
        let scheduled = |yaml: &str| {
            serde_saphyr::from_str::<Workflow>(yaml)
                .expect("workflow parses")
                .runs_on_schedule()
        };
        assert!(scheduled(
            "on:\n  schedule:\n    - cron: '0 0 * * *'\n  workflow_dispatch:\n"
        ));
        for yaml in [
            "name: Audit\n",
            "on: schedule\n",
            "on: [schedule, push]\n",
            "on:\n  schedule:\n  push:\n",
            "on:\n  schedule: []\n",
            "on:\n  workflow_dispatch:\n",
        ] {
            assert!(!scheduled(yaml), "{yaml}");
        }
    }

    fn job(yaml: &str) -> WorkflowJob {
        serde_saphyr::from_str(yaml).expect("job parses")
    }

    #[test]
    fn reads_needed_jobs() {
        assert_eq!(job("needs: lint\n").needed_jobs(), Ok(vec!["lint"]));
        assert_eq!(
            job("needs: [lint, test]\n").needed_jobs(),
            Ok(vec!["lint", "test"])
        );
        assert_eq!(job("steps: []\n").needed_jobs(), Ok(Vec::new()));
        assert_eq!(job("needs:\n").needed_jobs(), Ok(Vec::new()));
        assert_eq!(
            job("needs: [lint, [test]]\n").needed_jobs(),
            Err("needs lists [\"test\"], which isn't a job ID".to_owned())
        );
        assert_eq!(
            job("needs:\n  lint: true\n").needed_jobs(),
            Err("needs is {\"lint\":true}, not a job ID or a list of them".to_owned())
        );
    }

    #[test]
    fn runs_always_only_with_always() {
        for condition in [
            "always()",
            "${{ always() }}",
            "  ALWAYS( ) ",
            "${{always()}}",
        ] {
            assert!(
                job(&format!("if: {condition:?}\n")).runs_always(),
                "{condition}"
            );
        }
        for yaml in [
            "steps: []\n",
            "if: true\n",
            "if: '!cancelled()'\n",
            "if: success()\n",
            "if: always() && github.event_name == 'push'\n",
            "if: [always()]\n",
            "if: always(\n",
        ] {
            assert!(!job(yaml).runs_always(), "{yaml}");
        }
    }

    #[test]
    fn finds_results_the_gate_does_not_fail_on() {
        let gate = |steps: &str| job(&format!("steps:\n{steps}")).results_not_failing();
        assert_eq!(
            gate(
                "  - if: contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped')\n    run: exit 1\n"
            ),
            Vec::<&str>::new()
        );
        // Split across steps, in other cases and spellings.
        assert_eq!(
            gate(concat!(
                "  - if: ${{ Contains(needs.*.result, 'FAILURE') }}\n    run: echo failed && exit 2\n",
                "  - if: contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped')\n",
                "    run: |\n      echo not all jobs succeeded\n      false\n",
            )),
            Vec::<&str>::new()
        );
        // A step that doesn't exit with an error, or whose condition doesn't hold for the result alone.
        assert_eq!(
            gate(concat!(
                "  - if: contains(needs.*.result, 'failure')\n    run: echo failed\n",
                "  - if: contains(needs.*.result, 'cancelled')\n    run: exit 0\n",
                "  - if: contains(needs.*.result, 'skipped') && github.event_name == 'push'\n    run: exit 1\n",
                "  - if: contains(needs.lint.result, 'skipped')\n    run: exit 1\n",
            )),
            ["failure", "cancelled", "skipped"]
        );
        // Indexes into needs, or a step that lets the job carry on when it fails.
        assert_eq!(
            gate(concat!(
                "  - if: contains(needs[0].result, 'failure') || contains(needs['*'].result, 'cancelled')\n",
                "    run: exit 1\n",
                "  - if: contains(needs.*.result, 'skipped')\n    run: exit 1\n    continue-on-error: true\n",
                "  - if: contains(needs.*.result, 'skipped')\n    run: exit 1\n",
                "    continue-on-error: ${{ github.event_name == 'push' }}\n",
            )),
            ["failure", "cancelled", "skipped"]
        );
        // Commands after a shell keyword or before a comment.
        assert_eq!(
            gate(concat!(
                "  - if: contains(needs.*.result, 'failure')\n",
                "    run: if [ -n \"$A\" ]; then exit 1; fi\n",
                "  - if: contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped')\n",
                "    run: exit 1  # not every job succeeded\n",
                "    continue-on-error: false\n",
            )),
            Vec::<&str>::new()
        );
        assert_eq!(
            gate("  - run: exit 1\n  - if: contains(needs.*.result, 'failure')\n    run: exit 1\n"),
            ["cancelled", "skipped"]
        );
        assert_eq!(
            job("steps: []\n").results_not_failing(),
            ["failure", "cancelled", "skipped"]
        );
    }

    #[test]
    fn parses_an_empty_workflow() {
        let workflow: Workflow = serde_saphyr::from_str("name: CI\n").expect("workflow parses");
        assert!(!workflow.uses_action("actions/checkout"));
        assert!(!workflow.runs_command("cargo test"));
    }
}

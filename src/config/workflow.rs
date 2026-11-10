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

use crate::report::Issue;

/// The ID of `ci.yaml`'s gate job, which succeeds only if every other job did.
pub(crate) const GATE_JOB: &str = "ci-success";

/// A value from a mapping such as `with` or `env` as the step receives it: a string, whether the YAML wrote it as one or
/// as a boolean or number. A null or collection is no value.
fn value_text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::String(value) => Some(Cow::Borrowed(value)),
        Value::Bool(value) => Some(Cow::Owned(value.to_string())),
        Value::Number(value) => Some(Cow::Owned(value.to_string())),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// The patterns of a push trigger's `branches` or `branches-ignore`, a list of them or a single one, as written. A
/// filter that isn't a pattern or a list of them fails, since GitHub rejects the workflow.
fn branch_patterns(patterns: &Value) -> Result<Vec<&str>, Issue> {
    match patterns {
        Value::String(pattern) => Ok(vec![pattern.as_str()]),
        Value::Array(patterns) => Ok(patterns.iter().filter_map(Value::as_str).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Err(Issue::fail(
            "push's branch filter isn't a list of patterns, which GitHub rejects",
        )),
    }
}

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
    /// The environment variables every job's steps get: a mapping, or an expression giving one, kept as YAML wrote it.
    pub env: Option<Value>,
    /// The defaults of every job's steps, such as `run.working-directory`, kept as YAML wrote them.
    pub defaults: Option<Value>,
    pub jobs: BTreeMap<String, WorkflowJob>,
}

/// A single job in a workflow.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct WorkflowJob {
    /// The condition under which the job runs: an expression, with or without `${{ }}`.
    #[serde(rename = "if")]
    pub condition: Option<Value>,
    /// Whether the workflow carries on, and can succeed, when the job fails: a boolean, or an expression giving one.
    #[serde(rename = "continue-on-error")]
    pub continue_on_error: Option<Value>,
    /// The environment variables the job's steps get, over the workflow's: a mapping, or an expression giving one, kept
    /// as YAML wrote it.
    pub env: Option<Value>,
    /// The defaults of the job's steps, over the workflow's, such as `run.working-directory`, kept as YAML wrote them.
    pub defaults: Option<Value>,
    /// The runner the job runs on: a label, a list of them, a group, or an expression giving one.
    #[serde(rename = "runs-on")]
    pub runs_on: Option<Value>,
    /// The container the job's steps run in, kept as YAML wrote it.
    pub container: Option<Value>,
    /// The service containers the job starts, kept as YAML wrote them.
    pub services: Option<Value>,
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
    /// The environment variables the step gets, over its job's and the workflow's: a mapping, or an expression giving
    /// one, kept as YAML wrote it.
    pub env: Option<Value>,
    /// The directory the step's script runs in, over its job's and the workflow's defaults.
    #[serde(rename = "working-directory")]
    pub working_directory: Option<Value>,
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
        value_text(self.with.get(name)?)
    }

    /// The step's script as lines, with each line that ends in a backslash joined to the next, or none if it runs no
    /// script.
    pub(crate) fn run_lines(&self) -> Vec<String> {
        self.run
            .as_deref()
            .map(join_continued_lines)
            .unwrap_or_default()
    }

    /// Whether the step uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref.
    pub(crate) fn uses_action(&self, action: &str) -> bool {
        self.uses
            .as_deref()
            .is_some_and(|uses| is_action(uses, action))
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
        self.steps
            .iter()
            .filter(move |step| step.uses_action(action))
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

    /// Whether the workflow runs for every pull request and every commit pushed to one: `on` names `pull_request`
    /// without filtering it by `branches` or `paths`, and any `types` it lists include `opened`, `synchronize` and
    /// `reopened`, the activity types it runs for by default. A workflow that doesn't run on pull requests at all fails,
    /// and one that skips some of them warns.
    pub(crate) fn runs_on_every_pull_request(&self) -> Result<(), Issue> {
        const FILTERS: [&str; 4] = ["branches", "branches-ignore", "paths", "paths-ignore"];
        const DEFAULT_TYPES: [&str; 3] = ["opened", "synchronize", "reopened"];

        let not_run = || Err(Issue::fail("Doesn't run on pull_request"));
        let config = match &self.on {
            Some(Value::String(event)) if event == "pull_request" => return Ok(()),
            Some(Value::Array(events)) if events.iter().any(|event| event == "pull_request") => {
                return Ok(());
            }
            Some(Value::Object(events)) => match events.get("pull_request") {
                Some(config) => config,
                None => return not_run(),
            },
            _ => return not_run(),
        };
        let Value::Object(config) = config else {
            return Ok(());
        };

        // GitHub rejects a workflow whose filter both includes and ignores branches, or paths.
        for (filter, ignore) in [("branches", "branches-ignore"), ("paths", "paths-ignore")] {
            if config.contains_key(filter) && config.contains_key(ignore) {
                return Err(Issue::fail(format!(
                    "pull_request sets both {filter} and {ignore}, which GitHub rejects"
                )));
            }
        }

        let mut problems = Vec::new();
        let filters: Vec<_> = FILTERS
            .into_iter()
            .filter(|filter| config.contains_key(*filter))
            .collect();
        if !filters.is_empty() {
            problems.push(format!("pull_request filters by {}", filters.join(", ")));
        }
        if let Some(types) = config.get("types") {
            let listed = |activity: &str| match types {
                Value::String(listed) => listed == activity,
                Value::Array(listed) => listed.iter().any(|listed| listed == activity),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
            };
            let missing: Vec<_> = DEFAULT_TYPES
                .into_iter()
                .filter(|activity| !listed(activity))
                .collect();
            // Without any of them, it never runs on the commits of an open pull request.
            if missing.len() == DEFAULT_TYPES.len() {
                return Err(Issue::fail(format!(
                    "pull_request types omit {}, so it never runs on a pull request's commits",
                    missing.join(", ")
                )));
            }
            if !missing.is_empty() {
                problems.push(format!("pull_request types omit {}", missing.join(", ")));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Issue::warn(problems.join("; ")))
        }
    }

    /// The value of the environment variable `name` that `step` of `job` gets from the workflow: the step's own, or else
    /// its job's, or else the workflow's. An `env` given by an expression sets nothing that can be read.
    pub(crate) fn env<'a>(
        &'a self,
        job: &'a WorkflowJob,
        step: &'a WorkflowStep,
        name: &str,
    ) -> Option<Cow<'a, str>> {
        [&step.env, &job.env, &self.env]
            .into_iter()
            .find_map(|env| env.as_ref()?.as_object()?.get(name))
            .and_then(value_text)
    }

    /// Whether the script of `step` of `job` runs at the root of the repository: neither the step nor the job's or
    /// workflow's `defaults.run` sets a `working-directory` other than `.`.
    pub(crate) fn runs_at_root(&self, job: &WorkflowJob, step: &WorkflowStep) -> bool {
        let default = |defaults: Option<&Value>| {
            defaults
                .and_then(|defaults| defaults.get("run"))
                .and_then(|run| run.get("working-directory"))
                .cloned()
        };
        let directory = step
            .working_directory
            .clone()
            .or_else(|| default(job.defaults.as_ref()))
            .or_else(|| default(self.defaults.as_ref()));
        directory
            .as_ref()
            .is_none_or(|directory| matches!(directory.as_str(), Some("." | "./")))
    }

    /// Whether the workflow runs on pushes to each of `branches`, naming each in its filter: `on` names `push`, either
    /// without a branch filter or with a `branches` list that has each branch's name as one of its patterns. Patterns
    /// are compared as written rather than matched as globs, whose syntax GitHub reads differently from glob crates, so
    /// a glob such as `'**'` or a negated `'!dev'` names no branch, and `branches-ignore` warns. A `push`
    /// filtered only by `tags` or `tags-ignore` runs on no branch. A filter GitHub rejects, which stops the whole
    /// workflow from running, fails, and any other issue warns.
    pub(crate) fn runs_on_push_to(&self, branches: &[&str]) -> Result<(), Issue> {
        let not_run = || Err(Issue::warn("Doesn't run on push"));
        let config = match &self.on {
            Some(Value::String(event)) if event == "push" => return Ok(()),
            Some(Value::Array(events)) if events.iter().any(|event| event == "push") => {
                return Ok(());
            }
            Some(Value::Object(events)) => match events.get("push") {
                Some(config) => config,
                None => return not_run(),
            },
            _ => return not_run(),
        };
        let Value::Object(config) = config else {
            return Ok(());
        };

        let listed = match (config.get("branches"), config.get("branches-ignore")) {
            (Some(_), Some(_)) => {
                return Err(Issue::fail(
                    "push sets both branches and branches-ignore, which GitHub rejects",
                ));
            }
            (Some(patterns), None) => branch_patterns(patterns)?,
            (None, Some(patterns)) => {
                branch_patterns(patterns)?;
                return Err(Issue::warn(format!(
                    "push sets branches-ignore rather than naming {} in branches",
                    branches.join(", ")
                )));
            }
            (None, None) if config.contains_key("tags") || config.contains_key("tags-ignore") => {
                return Err(Issue::warn(
                    "push is filtered only by tags, so it runs on no branch",
                ));
            }
            (None, None) => return Ok(()),
        };

        let missing: Vec<_> = branches
            .iter()
            .copied()
            .filter(|branch| !listed.contains(branch))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Issue::warn(format!(
                "push's branches don't name {}",
                missing.join(", ")
            )))
        }
    }

    /// The entries of the workflow's `schedule`, if `on` maps it to a list. A `schedule` given only by name, as a string
    /// or in a list, sets no times and never runs.
    fn schedule(&self) -> &[Value] {
        match &self.on {
            Some(Value::Object(on)) => match on.get("schedule") {
                Some(Value::Array(entries)) => entries,
                _ => &[],
            },
            _ => &[],
        }
    }

    /// Whether the workflow runs on a schedule at least daily, a failure if it has none: some entry's `cron` runs on every day, with `*` for its day of the month, month and day
    /// of the week. A schedule that skips days warns.
    pub(crate) fn check_daily_schedule(&self) -> Result<(), Issue> {
        let schedule = self.schedule();
        if schedule.is_empty() {
            return Err(Issue::fail("No schedule"));
        }
        let daily = schedule.iter().any(|entry| {
            entry
                .get("cron")
                .and_then(Value::as_str)
                .is_some_and(|cron| {
                    let fields: Vec<&str> = cron.split_whitespace().collect();
                    fields.len() == 5
                        && fields
                            .get(2..)
                            .is_some_and(|days| days.iter().all(|field| *field == "*"))
                })
        });
        if daily {
            Ok(())
        } else {
            Err(Issue::warn("The schedule doesn't run every day"))
        }
    }

    /// Whether `on` names the event `event`: as itself, in a list, or as a key of a mapping.
    pub(crate) fn runs_on(&self, event: &str) -> bool {
        match &self.on {
            Some(Value::String(on)) => on == event,
            Some(Value::Array(on)) => on.iter().any(|name| name.as_str() == Some(event)),
            Some(Value::Object(on)) => on.contains_key(event),
            _ => false,
        }
    }

    /// The IDs of the jobs that set `continue-on-error` to anything but `false`: `true`, or an expression, which may
    /// give `true`. Such a job's failure doesn't fail the workflow, so a gate needing it, or the gate itself, can't be
    /// relied on to block a pull request.
    pub(crate) fn jobs_continuing_on_error(&self) -> Vec<&str> {
        self.jobs
            .iter()
            .filter(|(_, job)| {
                !matches!(
                    job.continue_on_error,
                    None | Some(Value::Null | Value::Bool(false))
                )
            })
            .map(|(id, _)| id.as_str())
            .collect()
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

    /// Every step that uses the action `action` (`owner/repo`, or `owner/repo/path`), at any ref, with the ID of its job.
    pub(crate) fn job_steps_using<'a>(
        &'a self,
        action: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a WorkflowStep)> {
        self.jobs
            .iter()
            .flat_map(move |(id, job)| job.steps_using(action).map(move |step| (id.as_str(), step)))
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

    /// The results of `f` for every command of every step's `run` script, split as [`commands`] splits a line, that it
    /// gives one for.
    pub(crate) fn filter_map_commands<T>(&self, mut f: impl FnMut(&[&str]) -> Option<T>) -> Vec<T> {
        let mut results = Vec::new();
        for line in self.run_lines() {
            results.extend(commands(&line).filter_map(|words| f(&words)));
        }
        results
    }

    /// Whether some step runs the formatter `name` so that it fails on unformatted files rather than rewriting them.
    /// `runs` tells, for a command's words, whether it runs the formatter at all, and if so how. A formatter that only
    /// rewrites passes every file, so it fails as surely as no formatter; one that checks with another flag than
    /// `--check` warns.
    pub(crate) fn check_formatter(
        &self,
        name: &str,
        runs: impl FnMut(&[&str]) -> Option<FormatterRun>,
    ) -> Result<(), Issue> {
        let runs = self.filter_map_commands(runs);
        if runs.is_empty() {
            Err(Issue::fail(format!("No step runs {name}")))
        } else {
            FormatterRun::check(
                &runs,
                name,
                Issue::fail(format!(
                    "{name} runs without --check, so it rewrites files instead of failing on them"
                )),
            )
        }
    }

    /// The issue of a workflow with no step using `action`, the form in which the standard runs `tool`: a warning if a
    /// step runs the tool itself instead, as `runs` tells from a command's words, since that works but isn't the
    /// template's form, or a failure if none does. The step running it isn't checked further.
    pub(crate) fn missing_action(
        &self,
        action: &str,
        tool: &str,
        mut runs: impl FnMut(&[&str]) -> bool,
    ) -> Issue {
        if self
            .run_lines()
            .any(|line| commands(&line).any(|words| runs(&words)))
        {
            Issue::warn(format!("A step runs {tool} rather than using {action}"))
        } else {
            Issue::fail(format!("No step uses {action}"))
        }
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

/// How a command runs a formatter, for [`Workflow::check_formatter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatterRun {
    /// It checks with `--check`, failing on unformatted files.
    Checks,
    /// It checks with another flag, such as `--diff`, which also fails on unformatted files without rewriting them.
    ChecksWith(&'static str),
    /// It rewrites unformatted files.
    Rewrites,
    /// It only prints the formatted files, as Prettier does without `--check` or `--write`, so it neither fails on
    /// unformatted files nor rewrites them.
    Prints,
}

impl FormatterRun {
    /// Whether `runs`, the runs of the formatter `name` in one place, such as a workflow, include one that checks with
    /// `--check`: if not, a warning if one checks with another flag, `rewrites`, the issue of a formatter that rewrites
    /// files, if one does, or else a failure, since a formatter that only prints passes every file.
    pub(crate) fn check(runs: &[Self], name: &str, rewrites: Issue) -> Result<(), Issue> {
        if runs.contains(&Self::Checks) {
            Ok(())
        } else if let Some(flag) = runs.iter().find_map(|run| match run {
            Self::ChecksWith(flag) => Some(flag),
            Self::Checks | Self::Rewrites | Self::Prints => None,
        }) {
            Err(Issue::warn(format!(
                "{name} runs with {flag} rather than --check"
            )))
        } else if runs.contains(&Self::Rewrites) {
            Err(rewrites)
        } else {
            Err(Issue::fail(format!(
                "{name} runs without --check or --write, so it only prints the formatted files and fails on none"
            )))
        }
    }

    /// Whether each of `runs`, the runs of the formatter `name` in a place that must check wherever it runs it, such as
    /// a script or a hook's copies, checks with `--check`: if not, the issue of the first that doesn't, as for
    /// [`check`](Self::check).
    pub(crate) fn check_each(runs: &[Self], name: &str, rewrites: Issue) -> Result<(), Issue> {
        runs.iter()
            .find(|&&run| run != Self::Checks)
            .map_or(Ok(()), |&run| Self::check(&[run], name, rewrites))
    }

    /// How a formatter given `args` runs: it checks if they include one of `checks`, checks otherwise if they include
    /// one of `others`, and rewrites if neither.
    pub(crate) fn from_args(args: &[&str], checks: &[&str], others: &[&'static str]) -> Self {
        if args.iter().any(|arg| checks.contains(arg)) {
            Self::Checks
        } else if let Some(flag) = others.iter().find(|flag| args.contains(flag)) {
            Self::ChecksWith(flag)
        } else {
            Self::Rewrites
        }
    }
}

/// The commands of a script line, split at the shell operators `&&`, `||`, `|` and `;`, each as its words.
pub(crate) fn commands(line: &str) -> impl Iterator<Item = Vec<&str>> {
    line.split(['&', '|', ';'])
        .map(|command| command.split_whitespace().collect())
}

/// Whether a word of a command names the program `name`: the program itself, a path to it, or a version of it as uvx
/// or npx takes it (`name@x.y.z` or `name==x.y.z`).
pub(crate) fn is_program(word: &str, name: &str) -> bool {
    let program = word.rsplit('/').next().unwrap_or(word);
    program.strip_prefix(name).is_some_and(|version| {
        version.is_empty() || version.starts_with('@') || version.starts_with("==")
    })
}

/// The arguments that follow the first word of a command that runs the program `name`, itself or through a path to it.
pub(crate) fn program_args<'a, 'b>(words: &'b [&'a str], name: &str) -> Option<&'b [&'a str]> {
    let start = words
        .iter()
        .position(|word| word.rsplit('/').next() == Some(name))?;
    words.get(start + 1..)
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
    fn programs_are_named_by_path_or_version() {
        for word in [
            "zizmor",
            "~/.cargo/bin/zizmor",
            "zizmor@1.32.0",
            "zizmor==1.32.0",
        ] {
            assert!(is_program(word, "zizmor"), "{word}");
        }
        for word in ["zizmorish", "zizmor-pre-commit", "pre-zizmor", "zizmor/bin"] {
            assert!(!is_program(word, "zizmor"), "{word}");
        }
    }

    #[test]
    fn program_args_follow_the_program_or_a_path_to_it() {
        assert_eq!(
            program_args(&["uv", "run", "pytest"], "pytest"),
            Some([].as_slice())
        );
        assert_eq!(
            program_args(&[".venv/bin/pytest", "-q"], "pytest"),
            Some(["-q"].as_slice())
        );
        assert_eq!(program_args(&["uv", "run", "pytest-cov"], "pytest"), None);
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
    fn a_missing_action_warns_on_a_step_running_the_tool() {
        let workflow = workflow();
        assert_eq!(
            workflow.missing_action("owner/test-action", "cargo test", |words| {
                words.starts_with(&["cargo", "test"])
            }),
            Issue::warn("A step runs cargo test rather than using owner/test-action")
        );
        // Only commands count: the step's name and the comment mention cargo deny, but no script runs it.
        assert_eq!(
            workflow.missing_action("EmbarkStudios/cargo-deny-action", "cargo deny", |words| {
                words.starts_with(&["cargo", "deny"])
            }),
            Issue::fail("No step uses EmbarkStudios/cargo-deny-action")
        );
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
    fn finds_jobs_continuing_on_error() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  plain:
    steps: []
  off:
    continue-on-error: false
  empty:
    continue-on-error:
  on:
    continue-on-error: true
  expression:
    continue-on-error: ${{ matrix.experimental }}
",
        )
        .expect("workflow parses");
        assert_eq!(workflow.jobs_continuing_on_error(), ["expression", "on"]);
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
        let unscheduled = |yaml: &str| {
            serde_saphyr::from_str::<Workflow>(yaml)
                .expect("workflow parses")
                .schedule()
                .is_empty()
        };
        assert!(!unscheduled(
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
            assert!(unscheduled(yaml), "{yaml}");
        }
    }

    #[test]
    fn audits_run_daily_and_on_dispatch() {
        let workflow =
            |yaml: &str| serde_saphyr::from_str::<Workflow>(yaml).expect("workflow parses");
        for cron in ["0 0 * * *", "30 6 * * *", "0 */6 * * *"] {
            assert_eq!(
                workflow(&format!("on:\n  schedule:\n    - cron: '{cron}'\n"))
                    .check_daily_schedule(),
                Ok(()),
                "{cron}"
            );
        }
        // One daily entry among others is enough.
        assert_eq!(
            workflow("on:\n  schedule:\n    - cron: '0 0 * * 1'\n    - cron: '0 12 * * *'\n")
                .check_daily_schedule(),
            Ok(())
        );
        for cron in ["0 0 * * 1", "0 0 1 * *", "0 0 * 1 *", "0 0 * *"] {
            assert_eq!(
                workflow(&format!("on:\n  schedule:\n    - cron: '{cron}'\n"))
                    .check_daily_schedule(),
                Err(Issue::warn("The schedule doesn't run every day")),
                "{cron}"
            );
        }
        assert_eq!(
            workflow("on:\n  workflow_dispatch:\n").check_daily_schedule(),
            Err(Issue::fail("No schedule"))
        );

        for yaml in [
            "on: workflow_dispatch\n",
            "on: [schedule, workflow_dispatch]\n",
            "on:\n  workflow_dispatch:\n",
        ] {
            assert!(workflow(yaml).runs_on("workflow_dispatch"), "{yaml}");
        }
        for yaml in ["name: Audit\n", "on: push\n", "on:\n  schedule: []\n"] {
            assert!(!workflow(yaml).runs_on("workflow_dispatch"), "{yaml}");
        }
    }

    #[test]
    fn runs_on_every_pull_request() {
        let runs = |yaml: &str| {
            serde_saphyr::from_str::<Workflow>(yaml)
                .expect("workflow parses")
                .runs_on_every_pull_request()
        };
        for yaml in [
            "on: pull_request\n",
            "on: [push, pull_request]\n",
            "on:\n  push:\n    branches: [main]\n  pull_request:\n",
            "on:\n  pull_request: {}\n",
            "on:\n  pull_request:\n    types: [opened, reopened, synchronize, labeled]\n",
        ] {
            assert_eq!(runs(yaml), Ok(()), "{yaml}");
        }
        for yaml in [
            "name: CI\n",
            "on: push\n",
            "on: [push, pull_request_target]\n",
            "on:\n  push:\n",
            "on:\n  push: ~\n",
        ] {
            assert_eq!(
                runs(yaml),
                Err(Issue::fail("Doesn't run on pull_request")),
                "{yaml}"
            );
        }
        assert_eq!(
            runs("on:\n  pull_request:\n    branches: [main]\n    paths-ignore: ['**.md']\n"),
            Err(Issue::warn(
                "pull_request filters by branches, paths-ignore"
            ))
        );
        assert_eq!(
            runs("on:\n  pull_request:\n    types: opened\n    paths: [src]\n"),
            Err(Issue::warn(
                "pull_request filters by paths; pull_request types omit synchronize, reopened"
            ))
        );
        for yaml in [
            "on:\n  pull_request:\n    types:\n",
            "on:\n  pull_request:\n    types: [closed, labeled]\n",
        ] {
            assert_eq!(
                runs(yaml),
                Err(Issue::fail(
                    "pull_request types omit opened, synchronize, reopened, so it never runs on a pull request's \
                     commits"
                )),
                "{yaml}"
            );
        }
        assert_eq!(
            runs("on:\n  pull_request:\n    branches: [main]\n    branches-ignore: [dev]\n"),
            Err(Issue::fail(
                "pull_request sets both branches and branches-ignore, which GitHub rejects"
            ))
        );
        assert_eq!(
            runs("on:\n  pull_request:\n    paths: [src]\n    paths-ignore: [docs]\n"),
            Err(Issue::fail(
                "pull_request sets both paths and paths-ignore, which GitHub rejects"
            ))
        );
    }

    #[test]
    fn runs_on_push_to_main_and_dev() {
        let runs = |yaml: &str| {
            serde_saphyr::from_str::<Workflow>(yaml)
                .expect("workflow parses")
                .runs_on_push_to(&["main", "dev"])
        };
        for yaml in [
            "on: push\n",
            "on: [push, pull_request]\n",
            "on:\n  push:\n",
            "on:\n  push:\n    branches: [dev, main]\n",
            "on:\n  push:\n    branches: [main, 'release/*', dev]\n",
            "on:\n  push:\n    branches: [main, dev]\n    tags: ['v*']\n",
            "on:\n  push:\n    paths: [src]\n",
        ] {
            assert_eq!(runs(yaml), Ok(()), "{yaml}");
        }
        let neither = Issue::warn("push's branches don't name main, dev");
        for (yaml, issue) in [
            ("on: pull_request\n", Issue::warn("Doesn't run on push")),
            ("name: CI\n", Issue::warn("Doesn't run on push")),
            ("on:\n  pull_request:\n", Issue::warn("Doesn't run on push")),
            (
                "on:\n  push:\n    branches: [main]\n",
                Issue::warn("push's branches don't name dev"),
            ),
            (
                "on:\n  push:\n    branches: main\n",
                Issue::warn("push's branches don't name dev"),
            ),
            (
                "on:\n  push:\n    branches: [main, 'd*']\n",
                Issue::warn("push's branches don't name dev"),
            ),
            ("on:\n  push:\n    branches: ['**']\n", neither.clone()),
            (
                "on:\n  push:\n    branches: ['*', '!release/*']\n",
                neither.clone(),
            ),
            ("on:\n  push:\n    branches: []\n", neither.clone()),
            ("on:\n  push:\n    branches: ['!main', '!dev']\n", neither),
            (
                "on:\n  push:\n    branches-ignore: ['renovate/**']\n",
                Issue::warn("push sets branches-ignore rather than naming main, dev in branches"),
            ),
            (
                "on:\n  push:\n    branches-ignore: dev\n",
                Issue::warn("push sets branches-ignore rather than naming main, dev in branches"),
            ),
            (
                "on:\n  push:\n    tags: ['v*']\n",
                Issue::warn("push is filtered only by tags, so it runs on no branch"),
            ),
            (
                "on:\n  push:\n    branches: [main]\n    branches-ignore: [dev]\n",
                Issue::fail("push sets both branches and branches-ignore, which GitHub rejects"),
            ),
            (
                "on:\n  push:\n    branches: {main: true}\n",
                Issue::fail("push's branch filter isn't a list of patterns, which GitHub rejects"),
            ),
            (
                "on:\n  push:\n    branches-ignore: {main: true}\n",
                Issue::fail("push's branch filter isn't a list of patterns, which GitHub rejects"),
            ),
        ] {
            assert_eq!(runs(yaml), Err(issue), "{yaml}");
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

    #[test]
    fn formatters_must_run_in_check_mode() {
        let check = |run: &str| {
            serde_saphyr::from_str::<Workflow>(&format!(
                "jobs:\n  format:\n    steps:\n      - run: {run}\n"
            ))
            .expect("workflow parses")
            .check_formatter("fmt", |words| {
                Some(FormatterRun::from_args(
                    program_args(words, "fmt")?,
                    &["--check"],
                    &["--diff"],
                ))
            })
        };
        assert_eq!(check("fmt --check"), Ok(()));
        assert_eq!(check("fmt --diff && fmt --check"), Ok(()));
        assert_eq!(
            check("fmt . && fmt --diff"),
            Err(Issue::warn("fmt runs with --diff rather than --check"))
        );
        assert_eq!(
            check("fmt ."),
            Err(Issue::fail(
                "fmt runs without --check, so it rewrites files instead of failing on them"
            ))
        );
        assert_eq!(check("lint ."), Err(Issue::fail("No step runs fmt")));
    }
}

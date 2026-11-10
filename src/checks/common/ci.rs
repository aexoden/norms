// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of the GitHub Actions workflows that apply to every project: the common hooks' jobs, triggers, timeouts,
//! concurrency, uv pins, runners and the gate job.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::config::{
    GATE_JOB, PrecommitConfig, Workflow, WorkflowJob, WorkflowStep, commands, current_stage_name,
    is_program, program_name,
};
use crate::context::{AUDIT_WORKFLOW, CI_WORKFLOW, ConfigStatus, ProjectContext};
use crate::report::{Issue, Outcome, Report, file_exists};

//
// markdownlint
//

/// The globs that lint every Markdown file in the project, rather than only those at its root.
const MARKDOWN_GLOBS: [&str; 2] = ["**/*.md", "**/*.{md,markdown}"];

/// The action that runs markdownlint-cli2 in CI.
const MARKDOWNLINT_ACTION: &str = "DavidAnson/markdownlint-cli2-action";

/// Whether `workflow` runs markdownlint-cli2 on every Markdown file: a [`MARKDOWNLINT_ACTION`] step whose `globs`
/// include `**/*.md`, since the action otherwise lints only the files at the root. With no such step, a step running
/// markdownlint-cli2 itself warns.
fn markdownlint_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut steps = workflow.steps_using(MARKDOWNLINT_ACTION).peekable();
    if steps.peek().is_none() {
        return Err(
            workflow.missing_action(MARKDOWNLINT_ACTION, "markdownlint-cli2", |words| {
                words
                    .iter()
                    .any(|word| is_program(word, "markdownlint-cli2"))
            }),
        );
    }

    if steps.any(|step| {
        let separator = step.input("separator");
        let separator = separator.as_deref().unwrap_or("\n");
        step.input("globs").is_some_and(|globs| {
            globs
                .split(separator)
                .any(|glob| MARKDOWN_GLOBS.contains(&glob.trim()))
        })
    }) {
        Ok(())
    } else {
        Err(Issue::warn(
            "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
        ))
    }
}

//
// pre-commit in CI
//

/// A word of a shell command, with any quotes around it removed, that pins pre-commit to a version: as uvx takes it
/// (`pre-commit@x.y.z`), or as pip and `uvx --from` do (`pre-commit==x.y.z`, also after `--from=`). A version from a
/// shell variable counts, since its value can't be seen here.
static PRE_COMMIT_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?:^|[=/])pre-commit(?:@|==)(?:[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc|\.post|\.dev)[0-9]+)?$|\$)").unwrap()
});

/// Whether a word of a shell command pins pre-commit to a version.
fn is_pre_commit_pin(word: &str) -> bool {
    PRE_COMMIT_PIN.is_match(word.trim_matches(['"', '\'']))
}

/// Whether a word of a shell command, with any quotes around it removed, invokes pre-commit: `pre-commit`, a path to
/// it, or a versioned `pre-commit@x.y.z` as uvx takes it.
fn is_pre_commit_command(word: &str) -> bool {
    let word = word.trim_matches(['"', '\'']);
    let name = program_name(word);
    name == "pre-commit" || name.starts_with("pre-commit@")
}

/// Whether a word of a shell command assigns a variable for the command it precedes, as `SKIP=clippy` does.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether `workflow` runs every pre-commit hook it doesn't skip: a script runs `pre-commit run` for every hook of the
/// `pre-commit` stage (naming no hook, and with no `--hook-stage` or that stage's) with `--all-files` (or `-a`), with a
/// pinned pre-commit. The pin belongs to that command, as `uvx pre-commit@x.y.z` or `uvx --from pre-commit==x.y.z
/// pre-commit`, or, for a plain `pre-commit run`, perhaps after variable assignments such as `SKIP=...`, to an earlier
/// command of the same job that installs it, such as `pip install pre-commit==x.y.z`.
fn pre_commit_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut runs = false;
    let mut runs_all = false;
    let mut all_files = false;
    let mut pinned = false;

    for job in workflow.jobs.values() {
        let mut installed = false;
        for line in job.run_lines() {
            for words in commands(&line) {
                if !words
                    .windows(2)
                    .any(|pair| matches!(pair, [program, "run"] if is_pre_commit_command(program)))
                {
                    installed |= words.iter().any(|word| is_pre_commit_pin(word));
                    continue;
                }
                runs = true;
                let Some(run) = pre_commit_run_all(&words) else {
                    continue;
                };
                if current_stage_name(run.stage) != "pre-commit" {
                    continue;
                }
                runs_all = true;
                if !run.all_files {
                    continue;
                }
                all_files = true;
                let prefix = run.prefix;
                pinned |= words
                    .iter()
                    .take(prefix.len() + 1)
                    .any(|word| is_pre_commit_pin(word))
                    || (installed && prefix.iter().all(|word| is_assignment(word)));
            }
        }
    }

    if !runs {
        // pre-commit/action runs the hooks too, but with whatever pre-commit is newest.
        Err(if workflow.uses_action("pre-commit/action") {
            Issue::warn(
                "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned",
            )
        } else {
            Issue::fail("No step runs pre-commit run")
        })
    } else if !runs_all {
        Err(Issue::fail(
            "pre-commit run only runs named hooks or another --hook-stage's, not every pre-commit hook",
        ))
    } else if !all_files {
        // CI has no staged files, so without --all-files every hook is skipped and the job passes.
        Err(Issue::fail(
            "pre-commit run doesn't pass --all-files, so it checks no files",
        ))
    } else if !pinned {
        Err(Issue::warn(
            "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
        ))
    } else {
        Ok(())
    }
}

/// The options of `pre-commit run` that take a value, which isn't a hook ID.
const PRE_COMMIT_RUN_VALUE_OPTIONS: [&str; 20] = [
    "--checkout-type",
    "--color",
    "--commit-msg-filename",
    "--commit-object-name",
    "--config",
    "--from-ref",
    "--hook-stage",
    "--is-squash-merge",
    "--local-branch",
    "--origin",
    "--prepare-commit-message-source",
    "--remote-branch",
    "--remote-name",
    "--remote-url",
    "--rewrite-command",
    "--source",
    "--to-ref",
    "-c",
    "-o",
    "-s",
];

/// A command that runs `pre-commit run` for every hook of a stage, as [`pre_commit_run_all`] reads it.
struct PreCommitRun<'a, 'b> {
    /// The words before pre-commit, such as `uvx` or `SKIP=...`.
    prefix: &'b [&'a str],
    /// The stage, `--hook-stage`'s value or else `pre-commit`, without quotes around it.
    stage: &'a str,
    /// Whether it passes `--all-files` (or `-a`).
    all_files: bool,
}

/// The command, as its words, if it runs `pre-commit run` for every hook of a stage, however pre-commit is invoked, or
/// `None` if it doesn't run it or names the one hook it runs. Option values aren't hook IDs or other options, the words
/// after `--files` are files, and those after a `#` are a comment.
fn pre_commit_run_all<'a, 'b>(words: &'b [&'a str]) -> Option<PreCommitRun<'a, 'b>> {
    let start = words
        .windows(2)
        .position(|pair| matches!(pair, [program, "run"] if is_pre_commit_command(program)))?;
    let mut stage = "pre-commit";
    let mut all_files = false;
    let mut args = words.iter().skip(start + 2);
    while let Some(arg) = args.next() {
        if *arg == "--files" || arg.starts_with('#') {
            break;
        }
        if !arg.starts_with('-') {
            return None;
        }
        if matches!(*arg, "--all-files" | "-a") {
            all_files = true;
        } else if let Some(value) = arg.strip_prefix("--hook-stage=") {
            stage = value;
        } else if PRE_COMMIT_RUN_VALUE_OPTIONS.contains(arg) {
            let value = args.next();
            if *arg == "--hook-stage"
                && let Some(value) = value
            {
                stage = value;
            }
        }
    }
    Some(PreCommitRun {
        prefix: words.get(..start)?,
        stage: stage.trim_matches(['"', '\'']),
        all_files,
    })
}

/// The value a word of a shell command assigns to `SKIP`, as `SKIP=<value>` does, without quotes around it.
fn skip_assignment(word: &str) -> Option<&str> {
    word.strip_prefix("SKIP=")
        .map(|value| value.trim_matches(['"', '\'']))
}

/// The hooks whose jobs in `ci.yaml` may have other IDs than theirs: the job each of them gets in the standards.
const HOOK_JOBS: [(&str, &str); 2] = [
    ("markdownlint-cli2", "markdownlint"),
    ("tombi-format", "tombi"),
];

/// The hooks of `config` that run in `stage` and have a job of their own in `workflow`, other than `job_id`, as each
/// one's alias or ID, that `skip`, a comma-separated list of hook IDs and aliases as pre-commit reads `SKIP`, doesn't
/// list. A hook has a job if a job has its ID or alias as its own ID, or is the job [`HOOK_JOBS`] gives it.
fn hooks_not_skipped<'a>(
    config: &'a PrecommitConfig,
    workflow: &Workflow,
    job_id: &str,
    stage: &'a str,
    skip: &str,
) -> Vec<&'a str> {
    let skipped: BTreeSet<&str> = skip
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let has_job = |name: &str| {
        let mapped = HOOK_JOBS
            .iter()
            .find(|(hook, _)| *hook == name)
            .map(|(_, mapped)| *mapped);
        std::iter::once(name)
            .chain(mapped)
            .any(|job| job != job_id && workflow.jobs.contains_key(job))
    };
    let mut missing: Vec<&str> = Vec::new();
    for hook in config.hooks_in(stage) {
        let names: Vec<&str> = std::iter::once(hook.id.as_str())
            .chain(hook.alias())
            .collect();
        let label = hook.alias().unwrap_or(&hook.id);
        if names.iter().any(|name| has_job(name))
            && !names.iter().any(|name| skipped.contains(name))
            && !missing.contains(&label)
        {
            missing.push(label);
        }
    }
    missing
}

/// Check that each command of `workflow` that runs `pre-commit run` for every hook of a stage at the repository's
/// root, where `config` applies, lists in its `SKIP` every hook of `config` that runs in that stage and has a job of
/// its own, or
/// `None` if no command runs it. `SKIP` is read from the command's own `SKIP=` prefix, or else an earlier command of
/// the step that sets it (`SKIP=...` or `export SKIP=...`), or else the step's, job's or workflow's `env`.
fn pre_commit_skip(config: &PrecommitConfig, workflow: &Workflow) -> Option<Result<(), String>> {
    let mut found = false;
    let mut issues = Vec::new();
    for (job_id, job) in &workflow.jobs {
        for step in job
            .steps
            .iter()
            .filter(|step| workflow.runs_at_root(job, step))
        {
            let mut shell_skip: Option<String> = None;
            for line in step.run_lines() {
                for words in commands(&line) {
                    let Some(PreCommitRun { prefix, stage, .. }) = pre_commit_run_all(&words)
                    else {
                        let assignment = match words.as_slice() {
                            ["export", word] | [word] => skip_assignment(word),
                            _ => None,
                        };
                        if let Some(value) = assignment {
                            shell_skip = Some(value.to_owned());
                        }
                        continue;
                    };
                    found = true;
                    let skip = prefix
                        .iter()
                        .find_map(|word| skip_assignment(word))
                        .map(str::to_owned)
                        .or_else(|| shell_skip.clone())
                        .or_else(|| workflow.env(job, step, "SKIP").map(Cow::into_owned))
                        .unwrap_or_default();
                    let missing = hooks_not_skipped(config, workflow, job_id, stage, &skip);
                    let issue = format!(
                        "SKIP in {job_id} doesn't list these hooks, which have jobs of their own: {}",
                        missing.join(", ")
                    );
                    if !missing.is_empty() && !issues.contains(&issue) {
                        issues.push(issue);
                    }
                }
            }
        }
    }
    found.then(|| {
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    })
}

//
// zizmor
//

/// Whether the words of a command run zizmor with its pedantic persona, passing `--pedantic`.
pub(super) fn zizmor_command_pedantic(words: &[&str]) -> Option<bool> {
    let start = words.iter().position(|word| is_program(word, "zizmor"))?;
    Some(words.iter().skip(start + 1).any(|arg| *arg == "--pedantic"))
}

/// The action that runs zizmor in CI.
const ZIZMOR_ACTION: &str = "zizmorcore/zizmor-action";

/// Whether `workflow` runs zizmor's pedantic persona so that its findings fail the job: a [`ZIZMOR_ACTION`] step setting
/// `persona: pedantic` and turning off `advanced-security`, whose default uploads findings to code scanning and passes
/// the job.
fn zizmor_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut issues = Vec::new();

    for step in workflow.steps_using(ZIZMOR_ACTION) {
        let strict = step.input("persona").as_deref() == Some("pedantic");
        let gating = step.input("advanced-security").as_deref() == Some("false");
        match (strict, gating) {
            (true, true) => return Ok(()),
            (false, true) => {
                issues.push(Issue::warn("zizmor-action doesn't set persona: pedantic"));
            }
            // Uploading the findings to code scanning instead passes the job whatever zizmor finds.
            (true, false) => {
                issues.push(Issue::fail(
                    "zizmor-action doesn't set advanced-security: false",
                ));
            }
            (false, false) => issues.push(Issue::fail(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false",
            )),
        }
    }

    if issues.is_empty() {
        Err(workflow.missing_action(ZIZMOR_ACTION, "zizmor", |words| {
            words.iter().any(|word| is_program(word, "zizmor"))
        }))
    } else {
        issues.dedup();
        // Each issue is a step's, and the best of them decides: one that fails the job on findings, only with another
        // persona than pedantic, makes the check a warning whatever the others do.
        Issue::alternatives(issues)
    }
}

//
// Continuous integration
//

/// The action that checks REUSE compliance in CI.
const REUSE_ACTION: &str = "fsfe/reuse-action";

/// Whether `workflow` checks REUSE compliance with [`REUSE_ACTION`]. With no such step, a step running `reuse lint`
/// itself warns, but `reuse lint-file`, which lints only the files it's given, doesn't count, nor does `lint` after
/// reuse's own options.
fn reuse_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    if workflow.uses_action(REUSE_ACTION) {
        Ok(())
    } else {
        Err(
            workflow.missing_action(REUSE_ACTION, "reuse lint", |words| {
                words
                    .windows(2)
                    .any(|pair| matches!(pair, [program, "lint"] if is_program(program, "reuse")))
            }),
        )
    }
}

/// The action that installs uv.
pub(crate) const SETUP_UV: &str = "astral-sh/setup-uv";

/// An exact uv version, such as `0.9.5`.
static EXACT_UV_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap()
});

/// Whether `version`, a setup-uv step's `version` input, is an exact uv version.
pub(crate) fn is_exact_uv_version(version: &str) -> bool {
    EXACT_UV_VERSION.is_match(version)
}

/// Whether a job runs uv itself, such as `uv sync` or `uv run`, rather than only running tools through `uvx`: some
/// command's first word is `uv` or a path to it. A later word naming uv, as in `pip install uv` or `rm -rf ~/.cache/uv`,
/// doesn't run it.
fn job_runs_uv(job: &WorkflowJob) -> bool {
    job.run_lines().any(|line| {
        commands(&line).any(|words| words.first().is_some_and(|word| program_name(word) == "uv"))
    })
}

/// Check that every setup-uv step in a job that runs uv itself pins an exact uv version with its `version` input, or
/// `None` if there is no such step. Without the input, setup-uv installs the newest uv that `required-version` allows,
/// so the version CI runs changes with every uv release. A job that only runs tools through `uvx`, such as the
/// pre-commit job, doesn't install the project, so its uv needn't match.
fn setup_uv_exact(workflow: &Workflow) -> Option<Result<(), String>> {
    let mut used = false;
    let mut inexact = Vec::new();
    for (job, step) in workflow.job_steps_using(SETUP_UV) {
        if !workflow.jobs.get(job).is_some_and(job_runs_uv) {
            continue;
        }
        used = true;
        match step.input("version") {
            Some(version) if is_exact_uv_version(&version) => {}
            Some(version) => inexact.push(format!("{job} ({version})")),
            None if step.input("version-file").is_some() => {
                inexact.push(format!("{job} (version-file, not version)"));
            }
            None => inexact.push(format!("{job} (no version)")),
        }
    }
    used.then(|| {
        if inexact.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "setup-uv doesn't pin an exact uv version in: {}",
                inexact.join(", ")
            ))
        }
    })
}

/// `tombi format --check` in a CI script, which fails on unformatted files instead of rewriting them.
static TOMBI_FORMAT_CHECK: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)\btombi\s+format\b[^\n]*\s--check\b").unwrap()
});

pub(crate) fn check_github_actions(ctx: &ProjectContext, report: &mut Report) {
    let workflows_dir = ctx.path().join(".github/workflows");
    report.must(
        "ci-workflows-dir-exists",
        "CI: .github/workflows/ exists",
        file_exists(&workflows_dir),
    );

    if !workflows_dir.exists() {
        return;
    }

    report.require_parsed("ci-yaml", "CI: ci.yaml", ctx.ci_workflow());

    if let Some(audit) = report.require_parsed("audit-yaml", "CI: audit.yaml", ctx.audit_workflow())
    {
        report.check(
            "audit-yaml-schedule",
            "CI: audit.yaml: runs on a daily schedule",
            audit.check_daily_schedule(),
        );
        report.should(
            "audit-yaml-dispatch",
            "CI: audit.yaml: runs on manual dispatch (workflow_dispatch)",
            audit.runs_on("workflow_dispatch"),
        );
    }

    // Every other workflow must parse too. The checks above already cover ci.yaml and audit.yaml.
    for (name, status) in ctx
        .workflows()
        .iter()
        .filter(|(name, _)| *name != CI_WORKFLOW && *name != AUDIT_WORKFLOW)
    {
        report.must(
            "workflow-valid",
            format!("CI: {name}: valid"),
            match status {
                ConfigStatus::NotFound => Outcome::issue("File not found"),
                ConfigStatus::ParseError { error } => Outcome::issue(error.clone()),
                ConfigStatus::Ok(_) => Outcome::Pass,
            },
        );
    }

    for (name, workflow) in ctx
        .workflows()
        .iter()
        .filter_map(|(name, status)| Some((name, status.as_ref_ok()?)))
    {
        let untimed = workflow.jobs_without_timeout();
        report.should(
            "workflow-job-timeouts",
            format!("CI: {name}: every job sets timeout-minutes"),
            if untimed.is_empty() {
                Ok(())
            } else {
                Err(format!("No timeout-minutes: {}", untimed.join(", ")))
            },
        );
        report.should(
            "workflow-concurrency",
            format!("CI: {name}: concurrency cancels superseded runs"),
            workflow.cancels_superseded_runs(),
        );
        if let Some(exact) = setup_uv_exact(workflow) {
            report.should(
                "workflow-setup-uv-version",
                format!("CI: {name}: setup-uv pins an exact uv version in jobs running uv"),
                exact,
            );
        }
    }

    if let ConfigStatus::Ok(workflow) = ctx.ci_workflow() {
        check_ci_workflow(ctx, workflow, report);
    }
}

/// Check `ci.yaml`, `workflow`: the common hooks' jobs, its triggers and its gate job.
fn check_ci_workflow(ctx: &ProjectContext, workflow: &Workflow, report: &mut Report) {
    report.check("ci-reuse", "CI: REUSE compliance", reuse_in_ci(workflow));
    report.check(
        "ci-markdownlint",
        "CI: Markdown linting (every file)",
        markdownlint_in_ci(workflow),
    );
    report.must(
        "ci-tombi",
        "CI: TOML formatting",
        workflow.runs_matching(&TOMBI_FORMAT_CHECK),
    );
    report.check("ci-zizmor", "CI: zizmor (pedantic)", zizmor_in_ci(workflow));
    report.check(
        "ci-pre-commit",
        "CI: remaining pre-commit hooks (pinned, --all-files)",
        pre_commit_in_ci(workflow),
    );
    if let ConfigStatus::Ok(config) = ctx.precommit()
        && let Some(skip) = pre_commit_skip(config, workflow)
    {
        report.should(
            "ci-pre-commit-skip",
            "CI: the pre-commit job's SKIP lists every hook with a job of its own",
            skip,
        );
    }

    report.check(
        "ci-yaml-pull-request",
        "CI: ci.yaml: runs on every pull request",
        workflow.runs_on_every_pull_request(),
    );
    report.check(
        "ci-yaml-push",
        "CI: ci.yaml: runs on pushes to main and dev",
        workflow.runs_on_push_to(&["main", "dev"]),
    );

    check_slim_runner(workflow, report);
    check_gate(workflow, report);
}

/// The actions of the jobs that install nothing, each of which is the whole job apart from checking out the
/// repository: markdownlint's and zizmor's, and Tombi's setup action, which downloads a prebuilt binary.
const LIGHT_ACTIONS: [&str; 3] = [
    "DavidAnson/markdownlint-cli2-action",
    "tombi-toml/setup-tombi",
    "zizmorcore/zizmor-action",
];

/// The runner of the jobs that install nothing, which starts faster.
const SLIM_RUNNER: &str = "ubuntu-slim";

/// Whether `job` installs nothing, so that it belongs on [`SLIM_RUNNER`]: it is the gate job, or it uses one of
/// [`LIGHT_ACTIONS`] and otherwise only checks out the repository or runs `tombi`, with nothing else on the script line.
/// A job that runs in a container or starts services, which need Docker, or picks its runner with an expression, as an
/// OS matrix does, has its runner chosen for it.
fn is_light_job(id: &str, job: &WorkflowJob) -> bool {
    let runner_chosen = job.container.is_some()
        || job.services.is_some()
        || job
            .runs_on
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|runner| runner.contains("${{"));
    if runner_chosen {
        return false;
    }
    if id == GATE_JOB {
        return true;
    }
    let light = |step: &WorkflowStep| LIGHT_ACTIONS.iter().any(|action| step.uses_action(action));
    job.steps.iter().any(light)
        && job.steps.iter().all(|step| match (&step.uses, &step.run) {
            (Some(_), _) => light(step) || step.uses_action("actions/checkout"),
            (None, Some(_)) => step.run_lines().iter().all(|line| {
                commands(line).all(|words| {
                    words
                        .first()
                        .is_none_or(|word| *word == "tombi" || word.starts_with('#'))
                })
            }),
            (None, None) => true,
        })
}

/// Whether `job` runs on [`SLIM_RUNNER`]: its `runs-on` is that label, alone or as a list of one.
fn runs_on_slim(job: &WorkflowJob) -> bool {
    match &job.runs_on {
        Some(Value::String(runner)) => runner == SLIM_RUNNER,
        Some(Value::Array(runners)) => {
            matches!(runners.as_slice(), [runner] if runner == SLIM_RUNNER)
        }
        _ => false,
    }
}

/// Check that the jobs of `ci.yaml`, `workflow`, that install nothing run on [`SLIM_RUNNER`].
fn check_slim_runner(workflow: &Workflow, report: &mut Report) {
    let elsewhere: Vec<&str> = workflow
        .jobs
        .iter()
        .filter(|(id, job)| is_light_job(id, job) && !runs_on_slim(job))
        .map(|(id, _)| id.as_str())
        .collect();
    report.should(
        "ci-ubuntu-slim",
        format!("CI: jobs that install nothing run on {SLIM_RUNNER}"),
        if elsewhere.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "These jobs install nothing, but don't run on {SLIM_RUNNER}: {}",
                elsewhere.join(", ")
            ))
        },
    );
}

/// Check `ci.yaml`'s gate job, `ci-success`, the one check branch protection requires: it runs even when other jobs
/// fail, needs every other job, and fails unless each of them succeeded. Whether or not there is a gate, no job may set
/// `continue-on-error`, which lets the workflow pass when it fails.
fn check_gate(workflow: &Workflow, report: &mut Report) {
    let continuing = workflow.jobs_continuing_on_error();
    report.must(
        "ci-gate-continue-on-error",
        "CI: no job, the gate included, sets continue-on-error",
        if continuing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "continue-on-error lets the workflow pass when these jobs fail: {}",
                continuing.join(", ")
            ))
        },
    );

    let Some(gate) = workflow.jobs.get(GATE_JOB) else {
        report.must(
            "ci-gate-exists",
            format!("CI: gate job {GATE_JOB}"),
            Outcome::issue(format!("No {GATE_JOB} job")),
        );
        return;
    };
    report.must(
        "ci-gate-exists",
        format!("CI: gate job {GATE_JOB}"),
        Outcome::Pass,
    );

    report.must(
        "ci-gate-always",
        "CI: the gate runs whatever the other jobs' results",
        if gate.runs_always() {
            Ok(())
        } else {
            Err(format!(
                "{GATE_JOB} doesn't set if: always(), so it's skipped when a job fails, which passes a required check"
            ))
        },
    );

    report.must(
        "ci-gate-needs",
        "CI: the gate needs every other job",
        gate.needed_jobs().and_then(|needed| {
            let missing: Vec<_> = workflow
                .jobs
                .keys()
                .map(String::as_str)
                .filter(|id| *id != GATE_JOB && !needed.contains(id))
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("{GATE_JOB} doesn't need {}", missing.join(", ")))
            }
        }),
    );

    let not_failing = gate.results_not_failing();
    report.must(
        "ci-gate-results",
        "CI: the gate fails unless every job succeeded",
        if not_failing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "No step of {GATE_JOB} exits with an error when a job's result is {}, as one with \
                 if: contains(needs.*.result, '<result>') does",
                not_failing.join(" or ")
            ))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip(precommit: &str, workflow: &str) -> Option<Result<(), String>> {
        pre_commit_skip(
            &serde_saphyr::from_str(precommit).unwrap(),
            &serde_saphyr::from_str(workflow).unwrap(),
        )
    }

    #[test]
    fn pre_commit_skips_the_hooks_with_jobs() {
        let precommit = "\
default_stages: [pre-commit]
repos:
  - repo: local
    hooks:
      - {id: clippy}
      - {id: cargo-test}
      - {id: ruff-check, alias: ruff-check-api}
      - {id: markdownlint-cli2}
      - {id: tombi-format}
      - {id: check-yaml}
      - {id: conventional-pre-commit, stages: [commit-msg]}
";
        let workflow = |skip: &str| {
            format!(
                "\
env:
  SKIP: other
jobs:
  pre-commit:
    env:
      SKIP: {skip}
    steps:
      - uses: astral-sh/setup-uv@v7
      - run: uvx pre-commit@4.3.0 run --all-files
  clippy: {{}}
  cargo-test: {{}}
  ruff-check-api: {{}}
  markdownlint: {{}}
  tombi: {{}}
  conventional-pre-commit: {{}}
"
            )
        };
        assert_eq!(
            skip(
                precommit,
                &workflow("\"cargo-test, clippy,ruff-check,markdownlint-cli2,tombi-format\"")
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(
                precommit,
                &workflow("cargo-test,markdownlint,tombi-format")
            ),
            Some(Err(
                "SKIP in pre-commit doesn't list these hooks, which have jobs of their own: clippy, \
                 ruff-check-api, markdownlint-cli2"
                    .to_owned()
            ))
        );
        // The step's own SKIP replaces the job's, and the workflow's applies when neither sets one.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: clippy}]}]\n",
                "jobs:\n  clippy: {}\n  hooks:\n    env: {SKIP: clippy}\n    steps:\n      - run: pre-commit run -a\n        env: {SKIP: other}\n"
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                    .to_owned()
            ))
        );
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: clippy}]}]\n",
                "env: {SKIP: clippy}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n"
            ),
            Some(Ok(()))
        );
        // A hook named like the job running pre-commit has no job of its own.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: lint}]}]\n",
                "jobs:\n  lint:\n    steps:\n      - run: uvx pre-commit run --all-files\n"
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(
                precommit,
                "jobs:\n  clippy:\n    steps:\n      - run: cargo clippy\n"
            ),
            None
        );
    }

    #[test]
    fn pre_commit_skip_is_read_from_the_script() {
        let clippy =
            "repos: [{repo: local, hooks: [{id: clippy}, {id: cargo-test}, {id: clippy}]}]\n";
        let script = |env: &str, run: &str| {
            format!(
                "jobs:\n  clippy: {{}}\n  cargo-test: {{}}\n  hooks:\n    {env}steps:\n      - run: {run:?}\n"
            )
        };
        // pre-commit run naming one hook needn't skip any, so it isn't checked; the hook's ID follows the values of
        // options.
        for run in [
            "uvx pre-commit@4.3.0 run clippy --all-files",
            "pre-commit run --hook-stage pre-push clippy",
        ] {
            assert_eq!(skip(clippy, &script("", run)), None, "{run}");
        }
        // A run of another stage's hooks skips those of that stage that have jobs, given by its current or old name,
        // quoted or not.
        let stages = "default_stages: [pre-commit]\nrepos: [{repo: local, hooks: [{id: clippy, stages: [pre-commit, pre-push]}, {id: cargo-test}]}]\n";
        for run in [
            "pre-commit run --hook-stage commit-msg --commit-msg-filename title.txt",
            "pre-commit run --hook-stage=manual --all-files",
        ] {
            assert_eq!(skip(stages, &script("", run)), Some(Ok(())), "{run}");
        }
        for run in [
            "pre-commit run --hook-stage pre-push --all-files",
            "pre-commit run --hook-stage=push -a",
            "pre-commit run --hook-stage 'pre-push' -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                        .to_owned()
                )),
                "{run}"
            );
        }
        for run in [
            "pre-commit run --hook-stage pre-commit --all-files",
            "pre-commit run --hook-stage=commit -a",
            "pre-commit run --hook-stage \"pre-commit\" -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                        .to_owned()
                )),
                "{run}"
            );
        }
        // SKIP set in the script, as a prefix or by an earlier command, comes before env.
        for run in [
            "SKIP=clippy,cargo-test uvx pre-commit@4.3.0 run --all-files",
            "export SKIP='clippy,cargo-test'\npre-commit run -a",
            "SKIP=clippy,cargo-test; pre-commit run --files a.rs",
        ] {
            assert_eq!(
                skip(clippy, &script("env: {SKIP: other}\n    ", run)),
                Some(Ok(())),
                "{run}"
            );
        }
        // Each hook is named once, and an env given by an expression, which can't be read, sets no SKIP.
        assert_eq!(
            skip(
                clippy,
                &script("env: ${{ fromJSON(vars.ENV) }}\n    ", "pre-commit run -a")
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                    .to_owned()
            ))
        );
        // A job with the hook's own ID counts, as well as the job the standards give it.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: tombi-format}]}]\n",
                "jobs:\n  tombi-format: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n"
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: tombi-format"
                    .to_owned()
            ))
        );

        // pre-commit run in another directory runs another configuration.
        for elsewhere in [
            "jobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n        working-directory: ${{ runner.temp }}/project\n",
            "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: pre-commit run -a\n",
            "defaults: {run: {working-directory: frontend}}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n",
        ] {
            assert_eq!(skip(clippy, elsewhere), None, "{elsewhere}");
        }
        assert!(matches!(
            skip(
                clippy,
                "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: pre-commit run -a\n        working-directory: .\n"
            ),
            Some(Err(_))
        ));
    }

    #[test]
    fn slim_runner_as_a_label_or_list_of_one() {
        let slim = |yaml: &str| runs_on_slim(&serde_saphyr::from_str(yaml).unwrap());
        assert!(slim("runs-on: ubuntu-slim\n"));
        assert!(slim("runs-on: [ubuntu-slim]\n"));
        assert!(!slim("runs-on: ubuntu-latest\n"));
        assert!(!slim("runs-on: [ubuntu-slim, self-hosted]\n"));
        assert!(!slim("steps: []\n"));
    }

    #[test]
    fn light_jobs_install_nothing() {
        let light = |id: &str, yaml: &str| is_light_job(id, &serde_saphyr::from_str(yaml).unwrap());
        assert!(light(GATE_JOB, "steps:\n  - run: exit 1\n"));
        assert!(light(
            "markdownlint",
            "steps:\n  - uses: actions/checkout@v6\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
        assert!(light(
            "tombi",
            "steps:\n  - uses: actions/checkout@v6\n  - uses: tombi-toml/setup-tombi@v1\n  - run: tombi format --check\n"
        ));
        assert!(light(
            "zizmor",
            "steps:\n  - uses: zizmorcore/zizmor-action@v0\n"
        ));

        assert!(light(
            "tombi",
            "steps:\n  - uses: tombi-toml/setup-tombi@v1\n  - run: |\n      # Check formatting\n      tombi format --check\n"
        ));

        assert!(!light("lint", "steps:\n  - uses: actions/checkout@v6\n"));
        assert!(!light(
            "markdownlint",
            "services: {db: {image: postgres}}\nsteps:\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
        assert!(!light(
            "zizmor",
            "runs-on: ${{ matrix.os }}\nsteps:\n  - uses: zizmorcore/zizmor-action@v0\n"
        ));
        assert!(!light(
            GATE_JOB,
            "container: node:24\nsteps:\n  - run: exit 1\n"
        ));
        assert!(!light("zizmor", "steps:\n  - run: uvx zizmor .\n"));
        assert!(!light(
            "tombi",
            "steps:\n  - uses: tombi-toml/setup-tombi@v1\n  - run: tombi format --check && cargo fmt --check\n"
        ));
        assert!(!light(
            "markdownlint",
            "steps:\n  - uses: actions/setup-node@v6\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
    }

    fn pre_commit(run: &str) -> Result<(), Issue> {
        let yaml = format!("jobs:\n  hooks:\n    steps:\n      - run: {run:?}\n");
        pre_commit_in_ci(&serde_saphyr::from_str(&yaml).unwrap())
    }

    #[test]
    fn pre_commit_runs_every_hook_of_the_pre_commit_stage() {
        for run in [
            "uvx pre-commit@4.3.0 run cargo-test; echo --all-files",
            "uvx pre-commit@4.3.0 run cargo-test --all-files",
            "uvx pre-commit@4.3.0 run --hook-stage manual --all-files",
            "uvx pre-commit@4.3.0 run --hook-stage=pre-push -a",
            "uvx pre-commit@4.3.0 run -a --hook-stage manual",
        ] {
            assert_eq!(
                pre_commit(run),
                Err(Issue::fail(
                    "pre-commit run only runs named hooks or another --hook-stage's, not every pre-commit hook"
                )),
                "{run}"
            );
        }
    }

    #[test]
    fn pre_commit_runs_every_file_pinned() {
        for run in [
            "uvx pre-commit@4.3.0 run --all-files --show-diff-on-failure",
            "uvx \"pre-commit@$PRE_COMMIT_VERSION\" run --all-files",
            "uvx --from pre-commit==4.3.0 pre-commit run -a",
            "pip install pre-commit==4.3.0\npre-commit run --show-diff-on-failure --all-files",
            "uv tool install 'pre-commit==4.3.0' && pre-commit run \\\n  --all-files",
            "uvx --from=pre-commit==4.3.0rc1 pre-commit run -a",
            // The pre-commit stage, by its current or old name, is every hook's that runs at each commit.
            "uvx pre-commit@4.3.0 run --hook-stage pre-commit --all-files",
            "uvx pre-commit@4.3.0 run --hook-stage='commit' -a",
            // A run of another stage's hooks beside it doesn't matter.
            "uvx pre-commit@4.3.0 run --hook-stage manual --all-files\nuvx pre-commit@4.3.0 run -a",
            // Variables assigned for pre-commit don't hide the pin of an earlier install.
            "pip install pre-commit==4.3.0\nSKIP=clippy pre-commit run -a",
        ] {
            assert_eq!(pre_commit(run), Ok(()), "{run}");
        }
        for (run, issue) in [
            (
                "pre-commit install",
                Issue::fail("No step runs pre-commit run"),
            ),
            (
                "uvx pre-commit@4.3.0 run",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit@4.3.0 run --all-files-later",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit@4.3.0 run --hook-stage manual --all-files\nuvx pre-commit@4.3.0 run",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            // An option's value or a comment isn't an option.
            (
                "uvx pre-commit@4.3.0 run -c -a",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit@4.3.0 run # --all-files",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit run --all-files",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "uvx pre-commit@latest run --all-files",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "pip install pre-commit==4.3.0\nuvx pre-commit run -a",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "pip install pre-commit>=4 && pre-commit run -a",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
        ] {
            assert_eq!(pre_commit(run), Err(issue), "{run}");
        }

        // A pin in another job doesn't apply.
        let split: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - run: pip install pre-commit==4.3.0\n  b:\n    steps:\n      - run: pre-commit run -a\n",
        )
        .unwrap();
        assert!(pre_commit_in_ci(&split).is_err());
        let action: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - uses: pre-commit/action@v3.0.1\n",
        )
        .unwrap();
        assert_eq!(
            pre_commit_in_ci(&action),
            Err(Issue::warn(
                "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned"
            ))
        );
    }

    #[test]
    fn reuse_in_ci_needs_the_action() {
        let ci = |steps: &str| {
            reuse_in_ci(
                &serde_saphyr::from_str(&format!("jobs:\n  reuse:\n    steps:\n{steps}")).unwrap(),
            )
        };
        assert_eq!(
            ci(
                "      - uses: fsfe/reuse-action@676e2d560c9a403aa252096d99fcab3e1132b0f5 # v6.0.0\n"
            ),
            Ok(())
        );
        for run in [
            "reuse lint",
            "uvx reuse lint --json",
            "uvx reuse@6.0.0 lint",
            "uvx --from reuse reuse lint",
            "echo; reuse  lint",
        ] {
            assert_eq!(
                ci(&format!("      - run: {run}\n")),
                Err(Issue::warn(
                    "A step runs reuse lint rather than using fsfe/reuse-action"
                )),
                "{run}"
            );
        }
        for run in [
            "reuse lint-file src/main.rs",
            "nonreuse lint",
            "reuse --root . lint",
        ] {
            assert_eq!(
                ci(&format!("      - run: {run}\n")),
                Err(Issue::fail("No step uses fsfe/reuse-action")),
                "{run}"
            );
        }
    }

    #[test]
    fn setup_uv_pins_an_exact_version() {
        let workflow = |yaml: &str| -> Workflow { serde_saphyr::from_str(yaml).unwrap() };
        assert_eq!(
            setup_uv_exact(&workflow(
                "jobs:\n  test:\n    steps:\n      - run: uv sync\n"
            )),
            None
        );
        // Jobs that only run tools through uvx needn't pin uv.
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  pre-commit:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
        with:
          enable-cache: false
      - run: rm -rf ~/.cache/uv && pip install uv
      - run: uvx pre-commit@4.3.0 run --all-files
"
            )),
            None
        );
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  pre-commit:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
      - run: uvx pre-commit@4.3.0 run --all-files
  pytest:
    steps:
      - uses: astral-sh/setup-uv@abc
        with:
          version: 0.9.5
          python-version: \"3.12\"
      - run: uv run --locked pytest
"
            )),
            Some(Ok(()))
        );
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  audit:
    steps:
      - uses: astral-sh/setup-uv@v7
      - run: uv audit --locked
  lint:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: \">=0.9\"
      - run: uv --directory api run ruff check
  test:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: \"0.9\"
      - uses: astral-sh/setup-uv@v7
        with:
          version: \"0.9.5\"
      - run: cd api && uv sync --locked
  matrix:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: ${{ matrix.uv }}
      - run: uv lock --check
  file:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version-file: uv.toml
      - run: ~/.local/bin/uv run mypy .
"
            )),
            Some(Err(
                "setup-uv doesn't pin an exact uv version in: audit (no version), file (version-file, not version), \
                 lint (>=0.9), matrix (${{ matrix.uv }}), test (0.9)"
                    .to_owned()
            ))
        );
    }

    fn zizmor_in(yaml: &str) -> Result<(), Issue> {
        let workflow: Workflow = serde_saphyr::from_str(yaml).expect("workflow parses");
        zizmor_in_ci(&workflow)
    }

    #[test]
    fn zizmor_in_ci_needs_the_action() {
        let script = |run: &str| format!("jobs:\n  z:\n    steps:\n      - run: \"{run}\"\n");
        // Running zizmor itself works, but isn't the template's form, so its arguments aren't read.
        for run in [
            "zizmor --pedantic .",
            "uvx zizmor@1.32.0 .",
            "echo start\\n  ~/.cargo/bin/zizmor --persona auditor .",
        ] {
            assert_eq!(
                zizmor_in(&script(run)),
                Err(Issue::warn(
                    "A step runs zizmor rather than using zizmorcore/zizmor-action"
                )),
                "{run}"
            );
        }
        assert_eq!(
            zizmor_in(&script("echo zizmorish --pedantic")),
            Err(Issue::fail("No step uses zizmorcore/zizmor-action"))
        );
    }

    #[test]
    fn zizmor_action_must_be_pedantic_and_gating() {
        let step = |with: &str| {
            format!(
                "jobs:\n  z:\n    steps:\n      - uses: zizmorcore/zizmor-action@abc # v0.8.1\n        with: {{{with}}}\n"
            )
        };
        assert_eq!(
            zizmor_in(&step("persona: pedantic, advanced-security: false")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("persona: pedantic, advanced-security: \"false\"")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("persona: auditor, advanced-security: false")),
            Err(Issue::warn("zizmor-action doesn't set persona: pedantic"))
        );
        assert_eq!(
            zizmor_in(&step("advanced-security: false")),
            Err(Issue::warn("zizmor-action doesn't set persona: pedantic"))
        );
        assert_eq!(
            zizmor_in(&step("persona: pedantic")),
            Err(Issue::fail(
                "zizmor-action doesn't set advanced-security: false"
            ))
        );
        assert_eq!(
            zizmor_in(&step("")),
            Err(Issue::fail(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false"
            ))
        );

        // A step that fails the job on findings decides, though another uploads them instead.
        assert_eq!(
            zizmor_in(&format!(
                "{}      - uses: zizmorcore/zizmor-action@abc # v0.8.1\n        with: {{advanced-security: false}}\n",
                step("persona: pedantic")
            )),
            Err(Issue::warn(
                "zizmor-action doesn't set advanced-security: false; zizmor-action doesn't set persona: pedantic"
            ))
        );
        // The action decides, whatever another step runs.
        assert_eq!(
            zizmor_in(&format!(
                "{}      - run: zizmor --pedantic .\n",
                step("persona: pedantic")
            )),
            Err(Issue::fail(
                "zizmor-action doesn't set advanced-security: false"
            ))
        );
    }

    fn markdownlint(steps: &str) -> Result<(), Issue> {
        markdownlint_in_ci(
            &serde_saphyr::from_str(&format!("jobs:\n  markdownlint:\n    steps:\n{steps}"))
                .unwrap(),
        )
    }

    #[test]
    fn markdownlint_must_lint_every_file() {
        const ACTION: &str = "      - uses: DavidAnson/markdownlint-cli2-action@0000000000000000000000000000000000000000\n";
        for steps in [
            format!("{ACTION}        with:\n          globs: '**/*.md'\n"),
            format!(
                "{ACTION}        with:\n          globs: |\n            **/*.md\n            !vendor\n"
            ),
            format!(
                "{ACTION}        with:\n          globs: 'README.md;**/*.{{md,markdown}}'\n          separator: ';'\n"
            ),
            // A step that lints only the root files doesn't hide one that lints them all.
            format!("{ACTION}{ACTION}        with:\n          globs: '**/*.md'\n"),
        ] {
            assert_eq!(markdownlint(&steps), Ok(()), "{steps}");
        }
        for (steps, issue) in [
            (
                "      - run: echo markdownlint\n".to_owned(),
                Issue::fail("No step uses DavidAnson/markdownlint-cli2-action"),
            ),
            // Running markdownlint-cli2 itself works, but isn't the template's form, so its arguments aren't read.
            (
                "      - run: npx markdownlint-cli2 '**/*.md'\n".to_owned(),
                Issue::warn(
                    "A step runs markdownlint-cli2 rather than using DavidAnson/markdownlint-cli2-action",
                ),
            ),
            (
                "      - run: pnpm exec markdownlint-cli2@0.24.0 \"*.md\"\n".to_owned(),
                Issue::warn(
                    "A step runs markdownlint-cli2 rather than using DavidAnson/markdownlint-cli2-action",
                ),
            ),
            // The action decides, whatever another step runs.
            (
                format!("{ACTION}      - run: markdownlint-cli2 '**/*.md'\n"),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            (
                ACTION.to_owned(),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            (
                format!("{ACTION}        with:\n          globs: '*.md'\n"),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            (
                format!("{ACTION}        with:\n          globs: '**/*.md,docs'\n"),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
        ] {
            assert_eq!(markdownlint(&steps), Err(issue), "{steps}");
        }
    }

    #[test]
    fn tombi_format_must_check() {
        for run in [
            "tombi format --check",
            "tombi format --check --diagnostics-format github",
            "uvx tombi format . --check",
        ] {
            assert!(TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
        for run in [
            "tombi format",
            "tombi format\necho --check",
            "tombi lint --check",
            "tombi format --checked",
        ] {
            assert!(!TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
    }
}

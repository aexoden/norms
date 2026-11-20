// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of the GitHub Actions workflows that apply to every project: the common hooks' jobs, triggers, timeouts,
//! concurrency, uv pins, runners and the gate job.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::prek::tombi_format_run;
use crate::config::{
    Command, FormatterRun, GATE_JOB, GATE_NAME, PrekConfig, Workflow, WorkflowJob, WorkflowStep,
    commands, is_program, program_args, program_name,
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
// prek in CI
//

/// A word of a shell command that pins prek to a version: as uvx takes it (`prek@x.y.z`), or as pip and `uvx --from` do
/// (`prek==x.y.z`, also after `--from=`). A version from a shell variable counts, since its value can't be seen here.
static PREK_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(
        r"(?:^|[=/])prek(?:@|==)(?:[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc|\.post|\.dev)[0-9]+)?$|\$)",
    )
    .unwrap()
});

/// A tool that `taiki-e/install-action`'s `tool` input installs at a pinned version of prek: `prek@x.y.z`, or a version
/// from an expression, such as `prek@${{ env.PREK_VERSION }}`, whose value can't be seen here.
static PREK_TOOL_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"^prek@(?:[0-9]+(?:\.[0-9]+)*$|\$\{\{)").unwrap()
});

/// The action that installs prek in CI, as it does cargo-llvm-cov.
const INSTALL_ACTION: &str = "taiki-e/install-action";

/// The tools of [`INSTALL_ACTION`]'s `tool` input, which separates them with commas or whitespace.
fn install_action_tools(tools: &str) -> impl Iterator<Item = &str> {
    tools
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|tool| !tool.is_empty())
}

/// Whether a word of a shell command pins prek to a version.
fn is_prek_pin(word: &str) -> bool {
    PREK_PIN.is_match(word)
}

/// Whether `job` installs a pinned prek with [`INSTALL_ACTION`], among the tools of its `tool` input.
fn installs_pinned_prek(job: &WorkflowJob) -> bool {
    job.steps_using(INSTALL_ACTION).any(|step| {
        step.input("tool").is_some_and(|tools| {
            install_action_tools(&tools).any(|tool| PREK_TOOL_PIN.is_match(tool))
        })
    })
}

/// Whether a word of a shell command invokes prek: `prek`, a path to it, or a versioned `prek@x.y.z` as uvx takes it.
fn is_prek_command(word: &str) -> bool {
    let name = program_name(word);
    name == "prek" || name.starts_with("prek@")
}

/// Whether a word of a shell command assigns a variable for the command it precedes, as `PREK_SKIP=clippy` does.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether `workflow` runs every prek hook it doesn't skip: a script runs `prek run` for every hook of the `pre-commit`
/// stage (naming no hook, and with no `--stage` or that stage's) with `--all-files` (or `-a`), with a pinned prek. The
/// pin belongs to the job, which installs prek with [`INSTALL_ACTION`] at a pinned version, or to the command, as
/// `uvx prek@x.y.z` or `uvx --from prek==x.y.z prek`, or, for a plain `prek run`, perhaps after variable assignments
/// such as `PREK_SKIP=...`, to an earlier command of the same job that installs it, such as `pip install prek==x.y.z`.
fn prek_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut runs = false;
    let mut runs_all = false;
    let mut all_files = false;
    let mut pinned = false;

    for job in workflow.jobs.values() {
        let action = installs_pinned_prek(job);
        let mut installed = false;
        for line in job.run_lines() {
            for words in commands(&line).iter().map(Command::words) {
                if prek_run_start(&words).is_none() {
                    installed |= words.iter().any(|word| is_prek_pin(word));
                    continue;
                }
                runs = true;
                let Some(run) = prek_run_all(&words) else {
                    continue;
                };
                if run.stage != "pre-commit" {
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
                    .any(|word| is_prek_pin(word))
                    || ((action || installed) && prefix.iter().all(|word| is_assignment(word)));
            }
        }
    }

    if !runs {
        Err(if workflow.uses_action("j178/prek-action") {
            Issue::warn(
                "No step runs prek run, which CI runs from a pinned prek rather than with j178/prek-action",
            )
        } else {
            Issue::fail("No step runs prek run")
        })
    } else if !runs_all {
        Err(Issue::fail(
            "prek run only runs named hooks or another --stage's, not every pre-commit hook",
        ))
    } else if !all_files {
        // CI has no staged files, so without --all-files every hook is skipped and the job passes.
        Err(Issue::fail(
            "prek run doesn't pass --all-files, so it checks no files",
        ))
    } else if !pinned {
        Err(Issue::warn(format!(
            "prek isn't pinned to a version ({INSTALL_ACTION} with prek@x.y.z, or uvx prek@x.y.z)"
        )))
    } else {
        Ok(())
    }
}

/// The options of `prek run` that take a value, which isn't a hook ID.
const PREK_RUN_VALUE_OPTIONS: [&str; 33] = [
    "--cd",
    "--checkout-type",
    "--color",
    "--commit-msg-filename",
    "--commit-object-name",
    "--config",
    "--directory",
    "--files0-from",
    "--from-ref",
    "--glob",
    "--group",
    "--hide-status",
    "--hook-stage",
    "--is-squash-merge",
    "--local-branch",
    "--log-file",
    "--no-group",
    "--origin",
    "--prepare-commit-message-source",
    "--remote-branch",
    "--remote-name",
    "--remote-url",
    "--require-group",
    "--rewrite-command",
    "--skip",
    "--source",
    "--stage",
    "--to-ref",
    "-C",
    "-c",
    "-d",
    "-o",
    "-s",
];

/// The options of `prek run` that select its stage: `--stage`, and `--hook-stage`, pre-commit's name for it.
const PREK_STAGE_OPTIONS: [&str; 2] = ["--stage", "--hook-stage"];

/// A command that runs `prek run` for every hook of a stage, as [`prek_run_all`] reads it.
struct PrekRun<'a, 'b> {
    /// The words before prek, such as `uvx` or `PREK_SKIP=...`.
    prefix: &'b [&'a str],
    /// The stage, `--stage`'s value or else `pre-commit`. Unlike `prek.toml`, prek's command line doesn't take a
    /// stage's old name, such as `commit`.
    stage: &'a str,
    /// Whether it passes `--all-files` (or `-a`).
    all_files: bool,
}

/// The position of the word that invokes prek in the words of a command that runs `prek run`, however prek is invoked.
fn prek_run_start(words: &[&str]) -> Option<usize> {
    words
        .windows(2)
        .position(|pair| matches!(pair, [program, "run"] if is_prek_command(program)))
}

/// The stage an argument of `prek run` gives in itself, as `--stage=<stage>` does.
fn stage_assignment(arg: &str) -> Option<&str> {
    PREK_STAGE_OPTIONS
        .iter()
        .find_map(|option| arg.strip_prefix(option)?.strip_prefix('='))
}

/// The command, as its words, if it runs `prek run` for every hook of a stage, however prek is invoked, or `None` if it
/// doesn't run it or names the one hook it runs. Option values aren't hook IDs or other options, and the words after
/// `--files` are files.
fn prek_run_all<'a, 'b>(words: &'b [&'a str]) -> Option<PrekRun<'a, 'b>> {
    let start = prek_run_start(words)?;
    let mut stage = "pre-commit";
    let mut all_files = false;
    let mut args = words.iter().skip(start + 2);
    while let Some(arg) = args.next() {
        if *arg == "--files" {
            break;
        }
        if !arg.starts_with('-') {
            return None;
        }
        if matches!(*arg, "--all-files" | "-a") {
            all_files = true;
        } else if let Some(value) = stage_assignment(arg) {
            stage = value;
        } else if PREK_RUN_VALUE_OPTIONS.contains(arg) {
            let value = args.next();
            if PREK_STAGE_OPTIONS.contains(arg)
                && let Some(value) = value
            {
                stage = value;
            }
        }
    }
    Some(PrekRun {
        prefix: words.get(..start)?,
        stage,
        all_files,
    })
}

/// The value a word of a shell command assigns to `PREK_SKIP`, as `PREK_SKIP=<value>` does.
fn skip_assignment(word: &str) -> Option<&str> {
    word.strip_prefix("PREK_SKIP=")
}

/// The hooks whose jobs in `ci.yaml` may have other IDs than theirs: the job each of them gets in the standards.
const HOOK_JOBS: [(&str, &str); 2] = [
    ("markdownlint-cli2", "markdownlint"),
    ("tombi-format", "tombi"),
];

/// The hooks of `config` that run in `stage` and have a job of their own in `workflow`, other than `job_id`, as each
/// one's alias or ID, that `skip`, a comma-separated list of hook IDs and aliases as prek reads `PREK_SKIP`, doesn't
/// list. A hook has a job if a job has its ID or alias as its own ID, or is the job [`HOOK_JOBS`] gives it.
fn hooks_not_skipped<'a>(
    config: &'a PrekConfig,
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

/// A command of a workflow that runs `prek run` for every hook of a stage at the repository's root, where its
/// `prek.toml` applies.
struct RootPrekRun<'w> {
    /// The ID of the job that runs it.
    job_id: &'w str,
    /// The stage whose hooks it runs, as [`PrekRun`] gives it.
    stage: String,
    /// The `PREK_SKIP` it runs with: from the command's own `PREK_SKIP=` prefix, or else an earlier command of the step
    /// that sets it (`PREK_SKIP=...` or `export PREK_SKIP=...`), or else the step's, job's or workflow's `env`. prek
    /// reads `SKIP` only without `PREK_SKIP`, and norms accepts only `PREK_SKIP`, so `SKIP` counts for nothing here.
    skip: String,
}

/// Every command of `workflow` that runs `prek run` for every hook of a stage at the repository's root.
fn root_prek_runs(workflow: &Workflow) -> Vec<RootPrekRun<'_>> {
    let mut runs = Vec::new();
    for (job_id, job) in &workflow.jobs {
        for step in job
            .steps
            .iter()
            .filter(|step| workflow.runs_at_root(job, step))
        {
            let mut shell_skip: Option<String> = None;
            for line in step.run_lines() {
                for words in commands(&line).iter().map(Command::words) {
                    let Some(PrekRun { prefix, stage, .. }) = prek_run_all(&words) else {
                        let assignment = match words.as_slice() {
                            ["export", word] | [word] => skip_assignment(word),
                            _ => None,
                        };
                        if let Some(value) = assignment {
                            shell_skip = Some(value.to_owned());
                        }
                        continue;
                    };
                    let skip = prefix
                        .iter()
                        .find_map(|word| skip_assignment(word))
                        .map(str::to_owned)
                        .or_else(|| shell_skip.clone())
                        .or_else(|| workflow.env(job, step, "PREK_SKIP").map(Cow::into_owned))
                        .unwrap_or_default();
                    runs.push(RootPrekRun {
                        job_id,
                        stage: stage.to_owned(),
                        skip,
                    });
                }
            }
        }
    }
    runs
}

/// Check that each command of `workflow` that runs `prek run` for every hook of a stage at the repository's root,
/// where `config` applies, lists in its `PREK_SKIP` every hook of `config` that runs in that stage and has a job of
/// its own, or `None` if no command runs it.
fn prek_skip(config: &PrekConfig, workflow: &Workflow) -> Option<Result<(), String>> {
    let runs = root_prek_runs(workflow);
    let mut issues = Vec::new();
    for run in &runs {
        let missing = hooks_not_skipped(config, workflow, run.job_id, &run.stage, &run.skip);
        let issue = format!(
            "PREK_SKIP in {} doesn't list these hooks, which have jobs of their own: {}",
            run.job_id,
            missing.join(", ")
        );
        if !missing.is_empty() && !issues.contains(&issue) {
            issues.push(issue);
        }
    }
    (!runs.is_empty()).then(|| {
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    })
}

//
// norms
//

/// The norms hook that checks the whole project.
const NORMS_VERIFY_HOOK: &str = "norms-verify";

/// The arguments of `norms verify`, if the words of a command run it: `norms`, or a path to it, quoted or not, then
/// `verify`.
fn norms_verify_args<'a, 'b>(words: &'b [&'a str]) -> Option<&'b [&'a str]> {
    let start = words
        .windows(2)
        .position(|pair| matches!(pair, [program, "verify"] if program_name(program) == "norms"))?;
    words.get(start + 2..)
}

/// Whether the words of a command run `prek run` for the hook `id` alone, naming it.
fn prek_runs_hook(words: &[&str], id: &str) -> bool {
    let Some(start) = prek_run_start(words) else {
        return false;
    };
    let mut args = words.iter().skip(start + 2);
    while let Some(arg) = args.next() {
        if *arg == "--files" {
            return false;
        }
        if PREK_RUN_VALUE_OPTIONS.contains(arg) {
            args.next();
        } else if *arg == id {
            return true;
        }
    }
    false
}

/// Whether `job` checks out the repository with its whole history (`fetch-depth: 0`), and so its release tags.
fn checks_out_history(job: &WorkflowJob) -> bool {
    job.steps_using("actions/checkout")
        .any(|step| step.input("fetch-depth").as_deref() == Some("0"))
}

/// Whether `workflow` runs `norms verify` on the project as the standards require: in a job that runs it, as the
/// norms-verify hook (`prek run norms-verify`) or as `norms verify --deny-warnings`, with the whole history, which the
/// changelog check reads release tags from. A job that runs it otherwise warns, as does the prek job
/// running every hook of `config`, its norms-verify hook among them, at the repository's root, from a shallow
/// checkout. With none, it fails.
fn norms_verify_in_ci(config: Option<&PrekConfig>, workflow: &Workflow) -> Result<(), Issue> {
    let mut issues = Vec::new();
    for (job_id, job) in &workflow.jobs {
        for line in job.run_lines() {
            for words in commands(&line).iter().map(Command::words) {
                let mut problems = Vec::new();
                if let Some(args) = norms_verify_args(&words) {
                    if !args.contains(&"--deny-warnings") {
                        problems.push("without --deny-warnings");
                    }
                } else if !prek_runs_hook(&words, NORMS_VERIFY_HOOK) {
                    continue;
                }
                if !checks_out_history(job) {
                    problems.push("from a checkout without the whole history (fetch-depth: 0), so it sees no release tags");
                }
                if problems.is_empty() {
                    return Ok(());
                }
                issues.push(Issue::warn(format!(
                    "{job_id} runs norms verify {}",
                    problems.join(" and ")
                )));
            }
        }
    }

    let hook = config.is_some_and(|config| {
        config
            .hooks_in("pre-commit")
            .any(|hook| hook.id == NORMS_VERIFY_HOOK)
    });
    for run in root_prek_runs(workflow) {
        if hook
            && run.stage == "pre-commit"
            && !run
                .skip
                .split(',')
                .any(|name| name.trim() == NORMS_VERIFY_HOOK)
        {
            issues.push(Issue::warn(format!(
                "norms-verify runs only among the hooks of {}, rather than in a job of its own with the whole history",
                run.job_id
            )));
        }
    }

    issues.dedup();
    if issues.is_empty() {
        Err(Issue::fail(
            "No job runs norms verify, as the norms-verify hook or itself",
        ))
    } else {
        Issue::alternatives(issues)
    }
}

/// Whether the words of a command run the `commit-msg` hooks, as `prek run --stage commit-msg`.
fn runs_commit_msg_hooks(words: &[&str]) -> bool {
    prek_run_all(words).is_some_and(|run| run.stage == "commit-msg")
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
fn is_exact_uv_version(version: &str) -> bool {
    EXACT_UV_VERSION.is_match(version)
}

/// Whether a job runs uv itself, such as `uv sync` or `uv run`, rather than only running tools through `uvx`: some
/// command's first word is `uv` or a path to it. A later word naming uv, as in `pip install uv` or `rm -rf ~/.cache/uv`,
/// doesn't run it.
fn job_runs_uv(job: &WorkflowJob) -> bool {
    job.run_lines().any(|line| {
        commands(&line)
            .iter()
            .map(Command::words)
            .any(|words| words.first().is_some_and(|word| program_name(word) == "uv"))
    })
}

/// Check that every setup-uv step in a job that runs uv itself pins an exact uv version with its `version` input, or
/// `None` if there is no such step. Without the input, setup-uv installs the newest uv that `required-version` allows,
/// so the version CI runs changes with every uv release. A job that only runs tools through `uvx` doesn't install the
/// project, so its uv needn't match.
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

//
// Tool versions
//

/// What the shared preset's `customManagers:githubActionsVersions` updates, as Renovate's own pattern matches it: a
/// `# renovate:` comment naming a datasource and package, with only these optional fields and in this order, then after
/// whitespace a variable whose name ends in `_VERSION`, captured.
static RENOVATE_VERSION_VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(
        r"# renovate: datasource=[a-zA-Z0-9\-._]+? depName=\S+?(?: (?:lookupName|packageName)=\S+?)?(?: versioning=\S+?)?(?: extractVersion=\S+?)?(?: registryUrl=\S+?)?\s+([A-Za-z0-9_]+?_VERSION)\s*:",
    )
    .unwrap()
});

/// A package pinned to a version written out, as [`INSTALL_ACTION`]'s `tool` input takes it (`name@1.2.3`) or as uv
/// does (`name@1.2.3`, or `name==1.2.3`, perhaps with extras). A version from a variable or expression doesn't match.
static LITERAL_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._\-]*(?:\[[^\]]*\])?(?:@|==)[0-9]").unwrap()
});

/// The options of uv, and of its `run` and `tool` commands, that take the next word as their value (unless given as
/// `--option=value`), as uv 0.13's help lists them.
const UV_VALUE_OPTIONS: &[&str] = &[
    "--allow-insecure-host",
    "--build-constraints",
    "--cache-dir",
    "--color",
    "--config-file",
    "--config-setting",
    "--config-settings-package",
    "--constraints",
    "--default-index",
    "--directory",
    "--env-file",
    "--exclude-newer",
    "--exclude-newer-package",
    "--excludes",
    "--extra",
    "--extra-index-url",
    "--find-links",
    "--fork-strategy",
    "--from",
    "--group",
    "--index",
    "--index-strategy",
    "--index-url",
    "--keyring-provider",
    "--link-mode",
    "--no-binary-package",
    "--no-build-isolation-package",
    "--no-build-package",
    "--no-editable-package",
    "--no-extra",
    "--no-group",
    "--no-sources-package",
    "--only-group",
    "--overrides",
    "--package",
    "--prerelease",
    "--prerelease-package",
    "--project",
    "--python",
    "--python-platform",
    "--refresh-package",
    "--reinstall-package",
    "--resolution",
    "--torch-backend",
    "--upgrade-group",
    "--upgrade-package",
    "--with",
    "--with-editable",
    "--with-executables-from",
    "--with-requirements",
    "-C",
    "-P",
    "-b",
    "-c",
    "-f",
    "-i",
    "-p",
    "-w",
];

/// The options of uv's `run` and `tool` commands whose values are packages.
const UV_PACKAGE_OPTIONS: &[&str] = &["--from", "--with", "--with-executables-from", "-w"];

/// The options of [`UV_PACKAGE_OPTIONS`] whose values are comma-separated lists of packages, rather than one package.
const UV_PACKAGE_LIST_OPTIONS: &[&str] = &["--with", "--with-executables-from", "-w"];

/// The words of `args` from the first that isn't an option or an option's value, as `value_options` lists those that
/// take one, or those after a word that is only `--`, which ends the options.
fn skip_options<'a, 'b>(args: &'b [&'a str], value_options: &[&str]) -> &'b [&'a str] {
    let mut rest = args;
    while let [word, tail @ ..] = rest {
        if *word == "--" {
            return tail;
        }
        if !word.starts_with('-') {
            break;
        }
        rest = match tail {
            [_, after @ ..] if value_options.contains(word) => after,
            _ => tail,
        };
    }
    rest
}

/// The package specifications among a command's arguments, `args`: the values of the options `package_options` lists,
/// or each package of a value of one of `list_options` ([`split_packages`]), and, if `tool`, the command's first word
/// that isn't an option or the value of one `value_options` lists, or that follows a `--`, which ends the options. The
/// arguments after the tool (or after a `--` without `tool`) are left out. Short options written together (`-qp 3.14`)
/// aren't taken apart.
fn package_specs<'a>(
    args: &[&'a str],
    value_options: &[&str],
    (package_options, list_options): (&[&str], &[&str]),
    tool: bool,
) -> Vec<&'a str> {
    let mut specs = Vec::new();
    let mut words = args.iter().copied();
    while let Some(word) = words.next() {
        if word == "--" {
            specs.extend(words.next().filter(|_| tool));
            break;
        }
        if word.starts_with('-') {
            let (name, value) = match word.split_once('=') {
                Some((name, value)) => (name, package_options.contains(&name).then_some(value)),
                None if package_options.contains(&word) => (word, words.next()),
                None => {
                    if value_options.contains(&word) {
                        words.next();
                    }
                    (word, None)
                }
            };
            if list_options.contains(&name) {
                specs.extend(value.into_iter().flat_map(split_packages));
            } else {
                specs.extend(value);
            }
        } else {
            if tool {
                specs.push(word);
            }
            break;
        }
    }
    specs
}

/// The packages of a comma-separated list, as uv's `--with` takes them: split at the commas outside the brackets of
/// an extra, so that `x[a,b]==1.0` stays whole, each trimmed of whitespace, leaving out empty ones.
fn split_packages(list: &str) -> Vec<&str> {
    let mut packages = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    for (index, char) in list.char_indices() {
        match char {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                packages.extend(list.get(start..index));
                start = index + 1;
            }
            _ => {}
        }
    }
    packages.extend(list.get(start..));
    packages
        .into_iter()
        .map(str::trim)
        .filter(|package| !package.is_empty())
        .collect()
}

/// The package specifications of a command that runs a tool through uv, `uvx` or `uv tool run`, installs one with
/// `uv tool install`, or adds packages to `uv run` with `--with`, as [`package_specs`] finds them. uv's own options
/// may come before its command.
fn uv_package_specs<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let specs = |args, tool| {
        package_specs(
            args,
            UV_VALUE_OPTIONS,
            (UV_PACKAGE_OPTIONS, UV_PACKAGE_LIST_OPTIONS),
            tool,
        )
    };
    if let Some(args) = program_args(words, "uvx") {
        return specs(args, true);
    }
    let Some(args) = program_args(words, "uv") else {
        return Vec::new();
    };
    match skip_options(args, UV_VALUE_OPTIONS) {
        ["tool", args @ ..] => match skip_options(args, UV_VALUE_OPTIONS) {
            ["run" | "install", args @ ..] => specs(args, true),
            _ => Vec::new(),
        },
        ["run", args @ ..] => specs(args, false),
        _ => Vec::new(),
    }
}

/// A package pinned to a version written out, as npm and pnpm take it: `name@1.2.3` or `@scope/name@1.2.3`, perhaps as
/// a range (`name@^1.2`) or with a `v`. A version from a variable, or a tag such as `latest`, doesn't match.
static LITERAL_NPM_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"^(?:@[A-Za-z0-9][A-Za-z0-9._\-~]*/)?[A-Za-z0-9][A-Za-z0-9._\-~]*@[=^~<>]*v?[0-9]")
        .unwrap()
});

/// The options of npm and `npx` (`npm exec`) that take the next word as their value (unless given as
/// `--option=value`): the latter's own, as npm 11's help lists them, and npm's common configuration options.
const NPX_VALUE_OPTIONS: &[&str] = &[
    "--cache",
    "--call",
    "--loglevel",
    "--package",
    "--prefix",
    "--registry",
    "--userconfig",
    "--workspace",
    "-c",
    "-p",
    "-w",
];

/// The options of `npx` whose values are packages.
const NPX_PACKAGE_OPTIONS: &[&str] = &["--package", "-p"];

/// The options of pnpm and `pnpm dlx` that take the next word as their value (unless given as `--option=value`), as
/// pnpm 10's help lists them. pnpm's `-w` and `pnpm dlx`'s `-c` don't take one.
const PNPM_VALUE_OPTIONS: &[&str] = &[
    "--allow-build",
    "--dir",
    "--filter",
    "--loglevel",
    "--package",
    "--reporter",
    "-C",
    "-F",
];

/// The options of pnpm and `pnpm dlx` whose values are packages.
const PNPM_PACKAGE_OPTIONS: &[&str] = &["--package"];

/// The package specifications of a command that runs a package through `npx`, `npm exec`, `pnpm dlx` or `pnpx`, as
/// [`package_specs`] finds them. npm's and pnpm's own options may come before `exec` and `dlx`, with pnpm's `--package`
/// there too. A command `npx` runs in a shell (`-c`, `--call`) is left out, since its words aren't the command's own.
fn npm_package_specs<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let npx = |args: &[&'a str]| {
        let args = args
            .split(|word| matches!(*word, "-c" | "--call") || word.starts_with("--call="))
            .next()
            .unwrap_or_default();
        package_specs(args, NPX_VALUE_OPTIONS, (NPX_PACKAGE_OPTIONS, &[]), true)
    };
    if let Some(args) = program_args(words, "npx") {
        return npx(args);
    }
    if let Some(args) = program_args(words, "npm") {
        return match skip_options(args, NPX_VALUE_OPTIONS) {
            ["exec" | "x", args @ ..] => npx(args),
            _ => Vec::new(),
        };
    }
    let pnpm =
        |args, tool| package_specs(args, PNPM_VALUE_OPTIONS, (PNPM_PACKAGE_OPTIONS, &[]), tool);
    if let Some(args) = program_args(words, "pnpx") {
        return pnpm(args, true);
    }
    let Some(args) = program_args(words, "pnpm") else {
        return Vec::new();
    };
    match skip_options(args, PNPM_VALUE_OPTIONS) {
        ["dlx", dlx_args @ ..] => {
            let mut specs = pnpm(args, false);
            specs.extend(pnpm(dlx_args, true));
            specs
        }
        _ => Vec::new(),
    }
}

/// The options of Cargo itself, before its command, that take the next word as their value (unless given as
/// `--option=value`), as Cargo 1.100's help lists them.
const CARGO_VALUE_OPTIONS: &[&str] = &["--color", "--config", "--explain", "-C", "-Z"];

/// The options of `cargo install` that take the next word as their value (unless given as `--option=value`), as Cargo
/// 1.100's help lists them, besides `--version`. `cargo binstall` is read with these too, so the value of one of its own
/// options may be taken for a crate, which only names it in the warning.
const CARGO_INSTALL_VALUE_OPTIONS: &[&str] = &[
    "--bin",
    "--branch",
    "--color",
    "--config",
    "--example",
    "--features",
    "--git",
    "--index",
    "--jobs",
    "--message-format",
    "--path",
    "--profile",
    "--registry",
    "--rev",
    "--root",
    "--tag",
    "--target",
    "--target-dir",
    "-F",
    "-Z",
    "-j",
];

/// Whether a version requirement is written out, as `cargo install` takes it after `--version` or `name@` (`1.2.3`,
/// `=1.2.3`, `~1.2`, ...).
fn is_literal_cargo_version(version: &str) -> bool {
    version
        .trim_start_matches(['=', '^', '~', '<', '>'])
        .starts_with(|c: char| c.is_ascii_digit())
}

/// The crates a `cargo install` or `cargo binstall` command pins to a version written out, perhaps after a `+toolchain`
/// and Cargo's own options:
/// each written as `name@1.2.3`, and, with a `--version` (`--vers`) written out, the crates it names, as
/// `name@version`, or the option itself if it names none.
fn cargo_install_pins(words: &[&str]) -> Vec<String> {
    let Some(args) = program_args(words, "cargo") else {
        return Vec::new();
    };
    let args = match args {
        [toolchain, rest @ ..] if toolchain.starts_with('+') => rest,
        _ => args,
    };
    let ["install" | "binstall", args @ ..] = skip_options(args, CARGO_VALUE_OPTIONS) else {
        return Vec::new();
    };
    let mut crates = Vec::new();
    let mut version = None;
    let mut args = args.iter().copied();
    while let Some(word) = args.next() {
        if word == "--" {
            break;
        }
        if word.starts_with('-') {
            match word.split_once('=') {
                Some(("--version" | "--vers", value)) => {
                    version = Some(value);
                }
                Some(_) => {}
                None if matches!(word, "--version" | "--vers") => version = args.next(),
                None => {
                    if CARGO_INSTALL_VALUE_OPTIONS.contains(&word) {
                        args.next();
                    }
                }
            }
        } else {
            crates.push(word);
        }
    }

    let mut pins: Vec<String> = crates
        .iter()
        .filter(|name| {
            name.split_once('@')
                .is_some_and(|(_, pin)| is_literal_cargo_version(pin))
        })
        .map(|name| (*name).to_owned())
        .collect();
    if let Some(version) = version.filter(|version| is_literal_cargo_version(version)) {
        let unpinned: Vec<&str> = crates
            .iter()
            .copied()
            .filter(|name| !name.contains('@'))
            .collect();
        if unpinned.is_empty() && pins.is_empty() {
            pins.push(format!("--version {version}"));
        }
        pins.extend(unpinned.into_iter().map(|name| format!("{name}@{version}")));
    }
    pins
}

/// Whether an `env` value is a version written out, rather than empty or given by an expression.
fn is_literal_value(value: &Value) -> bool {
    match value {
        Value::String(value) => !value.is_empty() && !value.contains("${{"),
        Value::Number(_) => true,
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => false,
    }
}

/// Check that Renovate can update every tool version `workflow` pins where no manager of its own reads it: no version is
/// written inline, in [`INSTALL_ACTION`]'s `tool` input, a uv command, `cargo install` or `cargo binstall`, or `npx`,
/// `npm exec` or `pnpm dlx`, and every environment variable whose name ends in `_VERSION` and whose value is written
/// out has a `# renovate:` comment above it, as [`RENOVATE_VERSION_VARIABLE`] finds one. A variable set in several places needs a comment above each.
fn tool_versions(workflow: &Workflow) -> Result<(), String> {
    let mut inline: Vec<String> = Vec::new();
    let mut add = |id: &str, spec: &str| {
        let pin = format!("{id} ({spec})");
        if !inline.contains(&pin) {
            inline.push(pin);
        }
    };
    for (id, job) in &workflow.jobs {
        for tools in job
            .steps_using(INSTALL_ACTION)
            .filter_map(|step| step.input("tool"))
        {
            for tool in install_action_tools(&tools).filter(|tool| LITERAL_PIN.is_match(tool)) {
                add(id, tool);
            }
        }
        for line in job.run_lines() {
            for words in commands(&line).iter().map(Command::words) {
                for spec in uv_package_specs(&words)
                    .into_iter()
                    .filter(|spec| LITERAL_PIN.is_match(spec))
                {
                    add(id, spec);
                }
                for spec in npm_package_specs(&words)
                    .into_iter()
                    .filter(|spec| LITERAL_NPM_PIN.is_match(spec))
                {
                    add(id, spec);
                }
                for pin in cargo_install_pins(&words) {
                    add(id, &pin);
                }
            }
        }
    }

    // Each comment Renovate finds updates one variable, so a name set in several places needs as many.
    let mut comments: BTreeMap<&str, usize> = BTreeMap::new();
    for name in RENOVATE_VERSION_VARIABLE
        .captures_iter(&workflow.source)
        .filter_map(|captures| captures.get(1))
    {
        *comments.entry(name.as_str()).or_default() += 1;
    }
    let mut uncommented: BTreeSet<&str> = BTreeSet::new();
    for (name, _) in workflow.env_maps().flatten().filter(|(name, value)| {
        name.len() > "_VERSION".len() && name.ends_with("_VERSION") && is_literal_value(value)
    }) {
        match comments.get_mut(name.as_str()) {
            Some(count) if *count > 0 => *count -= 1,
            _ => {
                uncommented.insert(name);
            }
        }
    }

    let mut problems = Vec::new();
    if !inline.is_empty() {
        problems.push(format!(
            "Versions written inline, which Renovate doesn't update: {} (pin each in a _VERSION variable under a \
             # renovate: comment)",
            inline.join(", ")
        ));
    }
    if !uncommented.is_empty() {
        problems.push(format!(
            "No # renovate: comment, so Renovate doesn't update: {}",
            uncommented.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// Whether a command's words run `tombi format` with `--check`, as [`tombi_format_run`] tells.
fn tombi_format_checks(words: &[&str]) -> bool {
    tombi_format_run(words) == Some(FormatterRun::Checks)
}

/// The jobs `jobs` finds in each workflow that parsed, labeled with the workflow's name (`ci.yaml: test`), or `None`
/// if none parsed.
pub(crate) fn jobs_in_workflows<'a>(
    ctx: &'a ProjectContext,
    mut jobs: impl FnMut(&'a Workflow) -> Vec<&'a str>,
) -> Option<Vec<String>> {
    let mut parsed = false;
    let mut found = Vec::new();
    for (name, status) in ctx.workflows() {
        if let ConfigStatus::Ok(workflow) = status {
            parsed = true;
            found.extend(
                jobs(workflow)
                    .into_iter()
                    .map(|job| format!("{name}: {job}")),
            );
        }
    }
    parsed.then_some(found)
}

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
        .filter(|(name, _)| ![CI_WORKFLOW, AUDIT_WORKFLOW].contains(&name.as_str()))
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
        report.should(
            "workflow-tool-versions",
            format!("CI: {name}: Renovate can update every tool version"),
            tool_versions(workflow),
        );
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
        workflow.runs(tombi_format_checks),
    );
    report.check("ci-zizmor", "CI: zizmor (pedantic)", zizmor_in_ci(workflow));
    report.check(
        "ci-prek",
        "CI: remaining prek hooks (pinned, --all-files)",
        prek_in_ci(workflow),
    );
    report.check(
        "ci-norms-verify",
        "CI: norms verify (--deny-warnings, whole history)",
        norms_verify_in_ci(ctx.prek().as_ref_ok(), workflow),
    );
    report.must(
        "ci-commit-msg",
        "CI: commit-msg hooks on the commit message (prek run --stage commit-msg)",
        workflow.runs(runs_commit_msg_hooks),
    );
    if let ConfigStatus::Ok(config) = ctx.prek()
        && let Some(skip) = prek_skip(config, workflow)
    {
        report.should(
            "ci-prek-skip",
            "CI: the prek job's PREK_SKIP lists every hook with a job of its own",
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
                commands(line)
                    .iter()
                    .map(Command::words)
                    .all(|words| words.first().is_none_or(|word| *word == "tombi"))
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
        "ci-gate-name",
        format!("CI: the gate is named {GATE_NAME}"),
        check_gate_name(gate),
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

/// Check that the gate job is named `CI success`, the check branch protection requires: GitHub shows a job's check by
/// its `name`, or its ID without one, and a required check that never reports blocks every merge.
fn check_gate_name(gate: &WorkflowJob) -> Result<(), String> {
    match gate.name.as_ref() {
        Some(Value::String(name)) if name == GATE_NAME => Ok(()),
        Some(Value::String(name)) => Err(format!(
            "{GATE_JOB} is named {name}, but branch protection requires {GATE_NAME}"
        )),
        _ => Err(format!(
            "{GATE_JOB} has no name, so GitHub shows its check as {GATE_JOB}, but branch protection requires {GATE_NAME}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrekHook;

    fn skip(prek: &str, workflow: &str) -> Option<Result<(), String>> {
        prek_skip(
            &toml::from_str(prek).unwrap(),
            &serde_saphyr::from_str(workflow).unwrap(),
        )
    }

    fn norms_verify(workflow: &str) -> Result<(), Issue> {
        let config = PrekConfig::with_repo(
            "https://github.com/aexoden/norms",
            vec![PrekHook {
                args: vec!["--deny-warnings".to_owned()],
                ..PrekHook::new("norms-verify", None)
            }],
        );
        norms_verify_in_ci(Some(&config), &serde_saphyr::from_str(workflow).unwrap())
    }

    /// A job that checks out the repository, with `with` as the checkout's inputs, and runs `run`.
    fn norms_job(with: &str, run: &str) -> String {
        format!(
            "jobs:\n  norms-verify:\n    steps:\n      - uses: actions/checkout@v7\n        with: {with}\n      \
             - run: {run}\n"
        )
    }

    #[test]
    fn gate_is_named_as_branch_protection_requires() {
        let gate = |job: &str| check_gate_name(&serde_saphyr::from_str(job).unwrap());
        assert_eq!(gate("name: CI success\nsteps: []\n"), Ok(()));
        assert!(
            gate("name: Gate\nsteps: []\n")
                .unwrap_err()
                .contains("named Gate")
        );
        assert!(gate("steps: []\n").unwrap_err().contains("has no name"));
    }

    #[test]
    fn norms_verify_runs_as_the_hook_or_itself_with_history() {
        for run in [
            "uvx prek@0.5.11 run norms-verify --all-files",
            "uvx prek@0.5.11 run --all-files norms-verify",
            "'\"$RUNNER_TEMP/norms/norms\" verify --deny-warnings .'",
            "norms verify --deny-warnings .",
        ] {
            assert_eq!(
                norms_verify(&norms_job("{fetch-depth: 0}", run)),
                Ok(()),
                "{run}"
            );
        }
    }

    #[test]
    fn norms_verify_warns_without_history_or_deny_warnings() {
        assert_eq!(
            norms_verify(&norms_job(
                "{persist-credentials: false}",
                "uvx prek@0.5.11 run norms-verify --all-files"
            )),
            Err(Issue::warn(
                "norms-verify runs norms verify from a checkout without the whole history (fetch-depth: 0), so it \
                 sees no release tags"
            ))
        );
        assert_eq!(
            norms_verify(&norms_job("{fetch-depth: 1}", "norms verify .")),
            Err(Issue::warn(
                "norms-verify runs norms verify without --deny-warnings and from a checkout without the whole \
                 history (fetch-depth: 0), so it sees no release tags"
            ))
        );
        // Named hooks other than norms-verify, and option values, don't run it.
        assert_eq!(
            norms_verify(&norms_job(
                "{fetch-depth: 0}",
                "uvx prek run --stage norms-verify clippy"
            )),
            Err(Issue::fail(
                "No job runs norms verify, as the norms-verify hook or itself"
            ))
        );
    }

    #[test]
    fn prek_runs_a_hook_it_names_before_any_files() {
        for (run, runs) in [
            ("uvx prek@0.5.11 run norms-verify --all-files", true),
            ("uvx prek@0.5.11 run 'norms-verify' --all-files", true),
            ("uvx prek@0.5.11 run --color always norms-verify", true),
            ("uvx prek@0.5.11 run --files norms-verify", false),
            ("uvx prek@0.5.11 run --color norms-verify", false),
            ("uvx prek@0.5.11 run # norms-verify", false),
            ("echo norms-verify", false),
        ] {
            let commands = commands(run);
            let words = commands.first().map_or_else(Vec::new, Command::words);
            assert_eq!(prek_runs_hook(&words, NORMS_VERIFY_HOOK), runs, "{run}");
        }
    }

    #[test]
    fn norms_verify_in_the_prek_job_warns() {
        let workflow = |skip: &str| {
            format!(
                "jobs:\n  prek:\n    steps:\n      - uses: actions/checkout@v7\n      - run: PREK_SKIP={skip} uvx \
                 prek@0.5.11 run --all-files\n"
            )
        };
        assert_eq!(
            norms_verify(&workflow("clippy")),
            Err(Issue::warn(
                "norms-verify runs only among the hooks of prek, rather than in a job of its own with the whole \
                 history"
            ))
        );
        // Even with the whole history, the hook belongs in a job of its own.
        assert_eq!(
            norms_verify(&workflow("clippy").replace(
                "actions/checkout@v7\n",
                "actions/checkout@v7\n        with: {fetch-depth: 0}\n"
            )),
            Err(Issue::warn(
                "norms-verify runs only among the hooks of prek, rather than in a job of its own with the whole \
                 history"
            ))
        );
        assert_eq!(
            norms_verify(&workflow("clippy,norms-verify")),
            Err(Issue::fail(
                "No job runs norms verify, as the norms-verify hook or itself"
            ))
        );
        // Without the hook, the prek job doesn't run norms.
        assert_eq!(
            norms_verify_in_ci(None, &serde_saphyr::from_str(&workflow("clippy")).unwrap()),
            Err(Issue::fail(
                "No job runs norms verify, as the norms-verify hook or itself"
            ))
        );
        // A job of its own decides.
        assert_eq!(
            norms_verify(&format!(
                "{}  prek:\n    steps:\n      - run: uvx prek@0.5.11 run --all-files\n",
                norms_job(
                    "{fetch-depth: 0}",
                    "uvx prek@0.5.11 run norms-verify --all-files"
                )
            )),
            Ok(())
        );
    }

    #[test]
    fn commit_msg_hooks_run_as_prek_run_with_their_stage() {
        let runs = |run: &str| {
            serde_saphyr::from_str::<Workflow>(&format!(
                "jobs:\n  norms-verify:\n    steps:\n      - run: {run:?}\n"
            ))
            .unwrap()
            .runs(runs_commit_msg_hooks)
        };
        for run in [
            "prek run --stage commit-msg --commit-msg-filename \"$RUNNER_TEMP/commit-msg.txt\"",
            "uvx prek@0.5.11 run --hook-stage=commit-msg --commit-msg-filename msg.txt",
        ] {
            assert!(runs(run), "{run}");
        }
        // Every pre-commit hook, or one commit-msg hook by name, isn't every commit-msg hook.
        for run in [
            "prek run --all-files",
            "prek run --stage commit-msg norms-commit-msg --commit-msg-filename msg.txt",
        ] {
            assert!(!runs(run), "{run}");
        }
    }

    #[test]
    fn prek_skips_the_hooks_with_jobs() {
        let prek = r#"
default_stages = ["pre-commit"]

[[repos]]
repo = "local"
hooks = [
  {id = "clippy"},
  {id = "cargo-test"},
  {id = "ruff-check", alias = "ruff-check-api"},
  {id = "markdownlint-cli2"},
  {id = "tombi-format"},
  {id = "check-yaml"},
  {id = "conventional-pre-commit", stages = ["commit-msg"]},
]
"#;
        let workflow = |skip: &str| {
            format!(
                "\
env:
  PREK_SKIP: other
jobs:
  prek:
    env:
      PREK_SKIP: {skip}
    steps:
      - uses: astral-sh/setup-uv@v7
      - run: uvx prek@0.5.11 run --all-files
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
                prek,
                &workflow("\"cargo-test, clippy,ruff-check,markdownlint-cli2,tombi-format\"")
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(prek, &workflow("cargo-test,markdownlint,tombi-format")),
            Some(Err(
                "PREK_SKIP in prek doesn't list these hooks, which have jobs of their own: clippy, \
                 ruff-check-api, markdownlint-cli2"
                    .to_owned()
            ))
        );
        // The step's own PREK_SKIP replaces the job's, and the workflow's applies when neither sets one.
        assert_eq!(
            skip(
                "repos = [{repo = \"local\", hooks = [{id = \"clippy\"}]}]\n",
                "jobs:\n  clippy: {}\n  hooks:\n    env: {PREK_SKIP: clippy}\n    steps:\n      - run: prek run -a\n        env: {PREK_SKIP: other}\n"
            ),
            Some(Err(
                "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                    .to_owned()
            ))
        );
        assert_eq!(
            skip(
                "repos = [{repo = \"local\", hooks = [{id = \"clippy\"}]}]\n",
                "env: {PREK_SKIP: clippy}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: prek run -a\n"
            ),
            Some(Ok(()))
        );
        // A hook named like the job running prek has no job of its own.
        assert_eq!(
            skip(
                "repos = [{repo = \"local\", hooks = [{id = \"lint\"}]}]\n",
                "jobs:\n  lint:\n    steps:\n      - run: uvx prek run --all-files\n"
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(
                prek,
                "jobs:\n  clippy:\n    steps:\n      - run: cargo clippy\n"
            ),
            None
        );
    }

    #[test]
    fn skip_is_not_read() {
        let clippy =
            r#"repos = [{repo = "local", hooks = [{id = "clippy"}, {id = "cargo-test"}]}]"#;
        let script = |env: &str, run: &str| {
            format!(
                "jobs:\n  clippy: {{}}\n  cargo-test: {{}}\n  hooks:\n    {env}steps:\n      - run: {run:?}\n"
            )
        };
        // prek reads pre-commit's SKIP only without PREK_SKIP, and norms not at all.
        for (env, run) in [
            ("", "SKIP=clippy,cargo-test prek run -a"),
            ("env: {SKIP: 'clippy,cargo-test'}\n    ", "prek run -a"),
            (
                "env: {PREK_SKIP: other}\n    ",
                "SKIP=clippy,cargo-test prek run -a",
            ),
        ] {
            assert_eq!(
                skip(clippy, &script(env, run)),
                Some(Err(
                    "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                        .to_owned()
                )),
                "{run}"
            );
        }
    }

    #[test]
    fn prek_skip_is_read_from_the_script() {
        let clippy = r#"repos = [{repo = "local", hooks = [{id = "clippy"}, {id = "cargo-test"}, {id = "clippy"}]}]"#;
        let script = |env: &str, run: &str| {
            format!(
                "jobs:\n  clippy: {{}}\n  cargo-test: {{}}\n  hooks:\n    {env}steps:\n      - run: {run:?}\n"
            )
        };
        // prek run naming one hook needn't skip any, so it isn't checked; the hook's ID follows the values of
        // options.
        for run in [
            "uvx prek@0.5.11 run clippy --all-files",
            "prek run --stage pre-push clippy",
        ] {
            assert_eq!(skip(clippy, &script("", run)), None, "{run}");
        }
        // A run of another stage's hooks skips those of that stage that have jobs, quoted or not, which the hooks may
        // list by the stage's old name.
        let stages = r#"
default_stages = ["pre-commit"]
repos = [{repo = "local", hooks = [{id = "clippy", stages = ["pre-commit", "push"]}, {id = "cargo-test"}]}]
"#;
        for run in [
            "prek run --stage commit-msg --commit-msg-filename title.txt",
            "prek run --stage=manual --all-files",
        ] {
            assert_eq!(skip(stages, &script("", run)), Some(Ok(())), "{run}");
        }
        for run in [
            "prek run --stage pre-push --all-files",
            "prek run --hook-stage=pre-push -a",
            "prek run --stage 'pre-push' -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                        .to_owned()
                )),
                "{run}"
            );
        }
        for run in [
            "prek run --stage pre-commit --all-files",
            "prek run --hook-stage=pre-commit -a",
            "prek run --stage \"pre-commit\" -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                        .to_owned()
                )),
                "{run}"
            );
        }
        // PREK_SKIP set in the script, as a prefix or by an earlier command, comes before env.
        for run in [
            "PREK_SKIP=clippy,cargo-test uvx prek@0.5.11 run --all-files",
            "export PREK_SKIP='clippy,cargo-test'\nprek run -a",
            "PREK_SKIP=clippy,cargo-test; prek run --files a.rs",
        ] {
            assert_eq!(
                skip(clippy, &script("env: {PREK_SKIP: other}\n    ", run)),
                Some(Ok(())),
                "{run}"
            );
        }
        // Each hook is named once, and an env given by an expression, which can't be read, sets no PREK_SKIP.
        assert_eq!(
            skip(
                clippy,
                &script("env: ${{ fromJSON(vars.ENV) }}\n    ", "prek run -a")
            ),
            Some(Err(
                "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                    .to_owned()
            ))
        );
        // A job with the hook's own ID counts, as well as the job the standards give it.
        assert_eq!(
            skip(
                "repos = [{repo = \"local\", hooks = [{id = \"tombi-format\"}]}]\n",
                "jobs:\n  tombi-format: {}\n  hooks:\n    steps:\n      - run: prek run -a\n"
            ),
            Some(Err(
                "PREK_SKIP in hooks doesn't list these hooks, which have jobs of their own: tombi-format"
                    .to_owned()
            ))
        );

        // prek run in another directory runs another configuration.
        for elsewhere in [
            "jobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: prek run -a\n        working-directory: ${{ runner.temp }}/project\n",
            "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: prek run -a\n",
            "defaults: {run: {working-directory: frontend}}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: prek run -a\n",
        ] {
            assert_eq!(skip(clippy, elsewhere), None, "{elsewhere}");
        }
        assert!(matches!(
            skip(
                clippy,
                "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: prek run -a\n        working-directory: .\n"
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

    fn prek(run: &str) -> Result<(), Issue> {
        let yaml = format!("jobs:\n  hooks:\n    steps:\n      - run: {run:?}\n");
        prek_in_ci(&serde_saphyr::from_str(&yaml).unwrap())
    }

    const UNPINNED: &str = "prek isn't pinned to a version (taiki-e/install-action with prek@x.y.z, or uvx prek@x.y.z)";

    #[test]
    fn prek_is_pinned_by_install_action() {
        let installed = |tool: &str, job: &str| {
            let install = format!(
                "      - uses: taiki-e/install-action@v2\n        with:\n          tool: {tool:?}\n"
            );
            let run = "      - run: PREK_SKIP=clippy prek run --all-files\n";
            let jobs = if job == "prek" {
                format!("  prek:\n    steps:\n{install}{run}")
            } else {
                format!("  {job}:\n    steps:\n{install}  prek:\n    steps:\n{run}")
            };
            prek_in_ci(&serde_saphyr::from_str(&format!("jobs:\n{jobs}")).unwrap())
        };
        for tool in [
            "prek@0.5.11",
            "prek@${{ env.PREK_VERSION }}",
            "cargo-deny@0.20.2, prek@0.5.11",
            "cargo-deny@0.20.2 prek@0.5.11",
            "cargo-deny\nprek@${{ env.PREK_VERSION }}\n",
        ] {
            assert_eq!(installed(tool, "prek"), Ok(()), "{tool}");
        }
        for tool in ["prek", "prek@latest", "prek-other@1.0.0", "cargo-deny"] {
            assert_eq!(
                installed(tool, "prek"),
                Err(Issue::warn(UNPINNED)),
                "{tool}"
            );
        }
        // An install in another job doesn't apply.
        assert_eq!(
            installed("prek@0.5.11", "other"),
            Err(Issue::warn(UNPINNED))
        );
    }

    #[test]
    fn prek_runs_every_hook_of_the_pre_commit_stage() {
        for run in [
            "uvx prek@0.5.11 run cargo-test; echo --all-files",
            "uvx prek@0.5.11 run cargo-test --all-files",
            "uvx prek@0.5.11 run --stage manual --all-files",
            "uvx prek@0.5.11 run --stage=pre-push -a",
            "uvx prek@0.5.11 run -a --stage manual",
        ] {
            assert_eq!(
                prek(run),
                Err(Issue::fail(
                    "prek run only runs named hooks or another --stage's, not every pre-commit hook"
                )),
                "{run}"
            );
        }
    }

    #[test]
    fn prek_runs_every_file_pinned() {
        for run in [
            "uvx prek@0.5.11 run --all-files --show-diff-on-failure",
            "uvx \"prek@$PREK_VERSION\" run --all-files",
            "uvx --from prek==0.5.11 prek run -a",
            "pip install prek==0.5.11\nprek run --show-diff-on-failure --all-files",
            "uv tool install 'prek==0.5.11' && prek run \\\n  --all-files",
            "uvx --from=prek==0.5.11rc1 prek run -a",
            // The pre-commit stage is every hook's that runs at each commit.
            "uvx prek@0.5.11 run --stage pre-commit --all-files",
            "uvx prek@0.5.11 run --hook-stage='pre-commit' -a",
            // A run of another stage's hooks beside it doesn't matter.
            "uvx prek@0.5.11 run --stage manual --all-files\nuvx prek@0.5.11 run -a",
            // Variables assigned for prek don't hide the pin of an earlier install.
            "pip install prek==0.5.11\nSKIP=clippy prek run -a",
        ] {
            assert_eq!(prek(run), Ok(()), "{run}");
        }
        for (run, issue) in [
            ("prek install", Issue::fail("No step runs prek run")),
            (
                "uvx prek@0.5.11 run",
                Issue::fail("prek run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx prek@0.5.11 run --all-files-later",
                Issue::fail("prek run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx prek@0.5.11 run --stage manual --all-files\nuvx prek@0.5.11 run",
                Issue::fail("prek run doesn't pass --all-files, so it checks no files"),
            ),
            // An option's value or a comment isn't an option.
            (
                "uvx prek@0.5.11 run -c -a",
                Issue::fail("prek run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx prek@0.5.11 run # --all-files",
                Issue::fail("prek run doesn't pass --all-files, so it checks no files"),
            ),
            ("uvx prek run --all-files", Issue::warn(UNPINNED)),
            ("uvx prek@latest run --all-files", Issue::warn(UNPINNED)),
            (
                "pip install prek==0.5.11\nuvx prek run -a",
                Issue::warn(UNPINNED),
            ),
            (
                "pip install prek>=0.5 && prek run -a",
                Issue::warn(UNPINNED),
            ),
        ] {
            assert_eq!(prek(run), Err(issue), "{run}");
        }

        // A pin in another job doesn't apply.
        let split: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - run: pip install prek==0.5.11\n  b:\n    steps:\n      - run: prek run -a\n",
        )
        .unwrap();
        assert!(prek_in_ci(&split).is_err());
        let action: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - uses: j178/prek-action@v3.1.0\n",
        )
        .unwrap();
        assert_eq!(
            prek_in_ci(&action),
            Err(Issue::warn(
                "No step runs prek run, which CI runs from a pinned prek rather than with j178/prek-action"
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
  prek:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
        with:
          enable-cache: false
      - run: rm -rf ~/.cache/uv && pip install uv
      - run: uvx prek@0.5.11 run --all-files
"
            )),
            None
        );
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  prek:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
      - run: uvx prek@0.5.11 run --all-files
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
        let runs = |line: &str| {
            commands(line)
                .iter()
                .any(|command| tombi_format_checks(&command.words()))
        };
        for run in [
            "tombi format --check",
            "tombi format --check --diagnostics-format github",
            "uvx tombi format . --check",
        ] {
            assert!(runs(run), "{run}");
        }
        for run in [
            "tombi format",
            "tombi format && echo --check",
            "tombi lint --check",
            "tombi format --checked",
            "echo 'tombi format --check'",
            "tombi format # --check",
            "uvx --check tombi format",
        ] {
            assert!(!runs(run), "{run}");
        }
    }

    #[test]
    fn uv_package_specs_follow_uv_options_and_the_end_of_options() {
        for (command, specs) in [
            ("uvx -- ruff@0.1.0 check", vec!["ruff@0.1.0"]),
            ("uv tool run --offline -- ruff@0.1.0", vec!["ruff@0.1.0"]),
            ("uv -q run --with x==1.0 -- pytest", vec!["x==1.0"]),
            ("uv run -- pytest --with y==1.0", vec![]),
            ("uv --directory=sub tool run x@1.0", vec!["x@1.0"]),
            (
                "uv --directory sub --color never tool install y==2.0",
                vec!["y==2.0"],
            ),
            ("uv pip install x==1.0", vec![]),
            ("uvx --from 'x==1.0,!=1.0.1' x", vec!["x==1.0,!=1.0.1", "x"]),
            (
                "uv run -w 'x[a,b]==1.0, y' --with=z pytest",
                vec!["x[a,b]==1.0", "y", "z"],
            ),
        ] {
            let commands = commands(command);
            let words = commands.first().map_or_else(Vec::new, Command::words);
            assert_eq!(uv_package_specs(&words), specs, "{command}");
        }
    }

    fn tool_versions_of(yaml: &str) -> Result<(), String> {
        tool_versions(&Workflow::parse(yaml.to_owned()).unwrap())
    }

    #[test]
    fn tool_versions_pinned_in_commented_variables_pass() {
        assert_eq!(
            tool_versions_of(
                "
env:
  # renovate: datasource=pypi depName=prek
  PREK_VERSION: \"0.5.10\"
  # renovate: datasource=github-releases depName=x/y packageName=x/z versioning=semver extractVersion=^v(?<version>.*)$
  Y_VERSION: 1.2.3
  RUST_VERSION: ${{ vars.RUST }}
  EMPTY_VERSION: ''
  VERSION: 1.0.0
  NOT_A_VERSION_FILE: 1.0.0
jobs:
  test:
    env:
      # renovate: datasource=crate depName=cargo-llvm-cov

      CARGO_LLVM_COV_VERSION: 0.9.1
    steps:
      - uses: taiki-e/install-action@abc # v2.90.5
        with:
          tool: prek@${{ env.PREK_VERSION }}, cargo-llvm-cov@${{ env.CARGO_LLVM_COV_VERSION }}, just
      - run: |
          uvx \"prek@$PREK_VERSION\" run --all-files
          uvx --python 3.14 --from \"copier==$COPIER\" copier copy . out
          echo uvx-like@1.0.0 && pip install ruff==0.1.0
          uvx copier copy --data dep==1.2 gh:x/y@1.0 out
          uvx --with=\"jinja2-time\" -p python3.14 prek run  # was prek@0.5.10
          uv run --group lint --with \"ruff==$RUFF_VERSION\" ruff check --with x==1.0
          uv run -- pytest --with=y==1.0 && uv --directory sub tool list x==1.0
          cargo install cross --locked --version \"$CROSS_VERSION\" && cargo install --path . --root out
          cargo install --git https://github.com/x/y --tag v1.0.0 y && cargo build --features x@1.0
          cargo install cargo-hack@\"$HACK_VERSION\" # cargo install x@1.0
          npx eslint . && npx -y prettier@latest --check . && pnpm dlx \"create-vite@$VITE_VERSION\" app
          pnpm --dir frontend exec eslint@9.0.0 && pnpm install x@1.0.0 && npx -c 'tsc@5.0.0'
"
            ),
            Ok(())
        );
    }

    #[test]
    fn tool_versions_written_inline_warn() {
        let result = tool_versions_of(
            "
jobs:
  deny:
    steps:
      - uses: taiki-e/install-action@abc
        with:
          tool: cargo-deny@0.20.2,prek@${{ env.PREK_VERSION }}
      - uses: taiki-e/install-action@abc
        with:
          tool: |
            just cargo-hack@0.6.0
            cargo-nextest@0.9.0
  hooks:
    steps:
      - run: >-
          uvx 'prek@0.5.10' run -a &&
          uvx --from=copier==9.19.1 --with jinja2-time==0.2.0,plumbum copier copy . out
      - run: uv tool install ruff[extra]@0.14.0 && uv tool run zizmor@1.0 . && uv run pytest==8
      - run: uvx --from=copier==9.19.1 copier --version
  options:
    steps:
      - run: uvx -p python3.14 --index pytorch black@24.1.0 .
      - run: uv --quiet tool run zizmor@1.1 . && uv --directory sub tool --offline install mypy==1.0
      - run: uv run --python 3.14 --group test -w pytest-cov==6.0,pytest --with=hypothesis==6.0 pytest
      - run: uvx --from='copier==9.19.1' --with=\"jinja2-time==0.2.0\" copier copy . out
  cargo:
    steps:
      - run: cargo install cargo-hack --locked --version 0.6.0 && cargo +stable binstall cargo-nextest@0.9.0
      - run: cargo install --features x --vers='=1.0.0' cargo-a cargo-b
      - run: cargo install --git https://github.com/x/y --version ~2.1
  npm:
    steps:
      - run: npx -y prettier@3.6.2 --check . && npx --package=@biomejs/biome@2.0.0 -p x@^1.0 biome
      - run: pnpm --dir frontend dlx @vue/cli@v5.0.8 create && pnpx 'tsx@~4.0' script.ts
",
        )
        .unwrap_err();
        assert_eq!(
            result,
            "Versions written inline, which Renovate doesn't update: cargo (cargo-hack@0.6.0), \
             cargo (cargo-nextest@0.9.0), cargo (cargo-a@=1.0.0), cargo (cargo-b@=1.0.0), cargo (--version ~2.1), \
             deny (cargo-deny@0.20.2), deny (cargo-hack@0.6.0), deny (cargo-nextest@0.9.0), hooks (prek@0.5.10), \
             hooks (copier==9.19.1), hooks (jinja2-time==0.2.0), hooks (ruff[extra]@0.14.0), hooks (zizmor@1.0), \
             npm (prettier@3.6.2), npm (@biomejs/biome@2.0.0), npm (x@^1.0), npm (@vue/cli@v5.0.8), npm (tsx@~4.0), \
             options (black@24.1.0), options (zizmor@1.1), options (mypy==1.0), options (pytest-cov==6.0), \
             options (hypothesis==6.0), options (copier==9.19.1), options (jinja2-time==0.2.0) \
             (pin each in a _VERSION variable under a # renovate: comment)"
        );
    }

    #[test]
    fn tool_versions_follow_quotes_and_unbalanced_lines() {
        assert_eq!(
            tool_versions_of(
                "
jobs:
  shell:
    steps:
      - run: |
          echo 'a && uvx y@1.0' && echo \"don't; cargo install z@1.0\"
          echo don't && uvx 'ruff@0.1.0' check # cargo install q@1.0
"
            )
            .unwrap_err(),
            "Versions written inline, which Renovate doesn't update: shell (ruff@0.1.0) \
             (pin each in a _VERSION variable under a # renovate: comment)"
        );
    }

    #[test]
    fn cargo_install_pins_follow_cargo_options() {
        for (command, pins) in [
            (
                "cargo -q --color never install cargo-hack@0.6.0",
                vec!["cargo-hack@0.6.0"],
            ),
            (
                "cargo +nightly --locked install x --version 1.0",
                vec!["x@1.0"],
            ),
            (
                "cargo install cargo-hack@~0.6 x@=1.0.0 y@$Y z@latest",
                vec!["cargo-hack@~0.6", "x@=1.0.0"],
            ),
            (
                "cargo install --target x86_64-unknown-linux-musl --bin rg -F pcre2 -j 2 ripgrep --version 14.0.0",
                vec!["ripgrep@14.0.0"],
            ),
            (
                "cargo install --git https://github.com/x/y --example e --version 1.0",
                vec!["--version 1.0"],
            ),
            ("cargo --config x=1 build", vec![]),
        ] {
            let words: Vec<&str> = command.split_whitespace().collect();
            assert_eq!(cargo_install_pins(&words), pins, "{command}");
        }
    }

    #[test]
    fn tool_versions_without_a_renovate_comment_warn() {
        let result = tool_versions_of(
            "
env:
  # renovate: datasource=pypi depName=prek
  PREK_VERSION: 0.5.10
  # A comment of another kind.
  COPIER_VERSION: 9.19.1
  # renovate: datasource=pypi depName=x unknownField=y
  X_VERSION: 1.0.0
  # renovate: depName=y datasource=pypi
  Y_VERSION: 1.0.0
jobs:
  test:
    steps:
      - run: true
        env:
          NODE_VERSION: 24
          # A job's own value needs a comment of its own.
          PREK_VERSION: 0.5.9
",
        )
        .unwrap_err();
        assert_eq!(
            result,
            "No # renovate: comment, so Renovate doesn't update: COPIER_VERSION, NODE_VERSION, PREK_VERSION, X_VERSION, \
             Y_VERSION"
        );
    }

    #[test]
    fn tool_versions_read_quoted_words_whole() {
        assert_eq!(
            tool_versions_of(
                "
jobs:
  quoted:
    steps:
      - run: |
          uv run --with 'x[a,b]==1.0,y' pytest && npx -c 'tsc x@1.0' && echo 'uvx y@1.0 && z'
          cargo install \"cargo-hack\" --version \"0.6.0\"  # cargo install q@1.0
"
            )
            .unwrap_err(),
            "Versions written inline, which Renovate doesn't update: quoted (x[a,b]==1.0), quoted (cargo-hack@0.6.0) \
             (pin each in a _VERSION variable under a # renovate: comment)"
        );
    }

    #[test]
    fn package_lists_split_outside_extras() {
        for (list, packages) in [
            ("x", vec!["x"]),
            ("x,y==1.0", vec!["x", "y==1.0"]),
            ("x[a,b]==1.0,y[c],z", vec!["x[a,b]==1.0", "y[c]", "z"]),
            ("x],y", vec!["x]", "y"]),
            ("x, y==1.0,,", vec!["x", "y==1.0"]),
            ("", vec![]),
        ] {
            assert_eq!(split_packages(list), packages, "{list}");
        }
    }

    #[test]
    fn npm_package_specs_follow_npm_and_pnpm_options() {
        for (command, specs) in [
            (
                "pnpm --package=@angular/cli@17.0.0 dlx ng new app",
                vec!["@angular/cli@17.0.0", "ng"],
            ),
            (
                "pnpm -w dlx create-vite@5.0.0 app",
                vec!["create-vite@5.0.0"],
            ),
            ("pnpm --loglevel error dlx -c tsx@4.0.0", vec!["tsx@4.0.0"]),
            ("pnpm dlx x@1.2.3", vec!["x@1.2.3"]),
            ("pnpx --package y@1.0 y", vec!["y@1.0", "y"]),
            (
                "npx --registry https://r.example --prefix web prettier@3.6.2",
                vec!["prettier@3.6.2"],
            ),
            ("npx -- prettier@3.6.2 --check .", vec!["prettier@3.6.2"]),
            ("npx -p x@1.0 -c 'tsc --project y@1.0.0'", vec!["x@1.0"]),
            ("npx --call='tsc z@1.0'", vec![]),
            (
                "npm --prefix web exec -- eslint@9.0.0 .",
                vec!["eslint@9.0.0"],
            ),
            ("npm x --package=a@1.0 a", vec!["a@1.0", "a"]),
            ("npm install x@1.0.0 && pnpm exec x@1.0.0", vec![]),
        ] {
            let commands = commands(command);
            let found: Vec<_> = commands
                .iter()
                .flat_map(|each| npm_package_specs(&each.words()))
                .collect();
            assert_eq!(found, specs, "{command}");
        }
    }
}

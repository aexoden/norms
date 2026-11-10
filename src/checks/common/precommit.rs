// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of `.pre-commit-config.yaml` that apply to every project: its settings and the common hooks, including
//! the commit message hook.

use std::collections::BTreeSet;

use super::ci::zizmor_command_pedantic;
use crate::config::{PrecommitConfig, PrecommitHook, program_name};
use crate::context::ProjectContext;
use crate::report::{Issue, Report};

//
// Pre-commit
//

/// The hooks from pre-commit-hooks that every project runs.
const PRE_COMMIT_HOOKS: [&str; 11] = [
    "check-added-large-files",
    "check-case-conflict",
    "check-illegal-windows-names",
    "check-merge-conflict",
    "check-symlinks",
    "check-toml",
    "check-vcs-permalinks",
    "check-yaml",
    "detect-private-key",
    "end-of-file-fixer",
    "trailing-whitespace",
];

/// Whether each hook of the pre-commit-hooks set runs before each commit.
fn pre_commit_hooks_in(config: &PrecommitConfig) -> Result<(), Issue> {
    let mut missing = Vec::new();
    let mut issues = Vec::new();
    for id in PRE_COMMIT_HOOKS {
        if !config
            .repos
            .iter()
            .flat_map(|repo| &repo.hooks)
            .any(|hook| hook.id == id)
        {
            missing.push(id);
        } else if let Err(issue) = config.check_commit_hook(id) {
            issues.push(issue);
        }
    }
    if !missing.is_empty() {
        issues.insert(0, Issue::fail(format!("Missing: {}", missing.join(", "))));
    }
    Issue::combine(issues)
}

/// Whether a zizmor hook runs the pedantic persona, passing `--pedantic` in its entry or the args pre-commit appends to
/// it. A hook without an entry, as a remote hook may leave it to the hook's repository, is taken to run `zizmor`.
fn zizmor_hook_pedantic(hook: &PrecommitHook) -> bool {
    hook.filter_map_commands("zizmor", zizmor_command_pedantic)
        .contains(&true)
}

pub(crate) fn check_precommit(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed(
        "precommit-config",
        ".pre-commit-config.yaml",
        ctx.precommit(),
    ) else {
        return;
    };

    // Without both types, pre-commit install leaves the pre-commit or the commit-msg hooks uninstalled, so they never
    // run, and pre-commit rejects a list it can't read.
    report.must(
        "precommit-install-hook-types",
        "Pre-commit: default_install_hook_types includes pre-commit and commit-msg",
        config.check_default_install_hook_types(),
    );
    // With any other list, hooks without stages of their own run in other stages than once before each commit, and
    // pre-commit rejects a list it can't read, so it runs no hooks at all.
    report.check(
        "precommit-default-stages",
        "Pre-commit: default_stages is [pre-commit]",
        config.check_default_stages(),
    );
    report.should(
        "precommit-hook-ids-unique",
        "Pre-commit: each hook has a unique ID or alias",
        config.check_unique_hook_ids(),
    );
    report.check(
        "precommit-pre-commit-hooks",
        "Pre-commit: the common hooks from pre-commit-hooks",
        pre_commit_hooks_in(config),
    );
    report.check(
        "precommit-reuse",
        "Pre-commit: reuse hook",
        config.check_commit_hook("reuse"),
    );
    report.check(
        "precommit-markdownlint",
        "Pre-commit: markdownlint-cli2 hook",
        config.check_commit_hook("markdownlint-cli2"),
    );
    report.check(
        "precommit-tombi",
        "Pre-commit: tombi-format hook",
        config.check_commit_hook("tombi-format"),
    );
    report.check(
        "precommit-conventional-pre-commit",
        "Pre-commit: conventional-pre-commit hook (commit-msg stage, recognized types)",
        conventional_hook_in(config),
    );
    report.check(
        "precommit-zizmor",
        "Pre-commit: zizmor hook (pedantic)",
        config.commit_hooks("zizmor").and_then(|hooks| {
            if hooks.into_iter().any(zizmor_hook_pedantic) {
                Ok(())
            } else {
                Err(Issue::warn("The zizmor hook doesn't pass --pedantic"))
            }
        }),
    );
}

//
// Commit messages
//

/// The commit types STANDARDS.md recognizes.
const COMMIT_TYPES: [&str; 11] = [
    "feat", "fix", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore",
];

/// The repository of conventional-pre-commit, whose hook sets the `commit-msg` stage itself.
const CONVENTIONAL_PRE_COMMIT_REPO: &str = "github.com/compilerla/conventional-pre-commit";

/// The types conventional-pre-commit always allows, adding them to any list of types that names neither.
const CONVENTIONAL_TYPES: [&str; 2] = ["feat", "fix"];

/// The commit types a conventional-pre-commit hook allows, from the words of its command after the program: the
/// positional arguments. Without any, it allows its own defaults, which the standard doesn't rely on, since a new
/// release may change them. Of its options, only `--scopes` takes a value, and argparse accepts it abbreviated to any
/// prefix down to `--sc`.
///
/// pre-commit appends the commit message file, which argparse takes from the first run of positional arguments, so
/// types followed by an option leave the arguments after it unrecognized, and the hook fails on every commit.
fn conventional_commit_types<'a>(args: &[&'a str]) -> Result<BTreeSet<&'a str>, Issue> {
    let mut types = BTreeSet::new();
    let mut args = args.iter().copied();
    let mut options = true;
    while let Some(arg) = args.next() {
        if options && arg.starts_with('-') {
            if !types.is_empty() {
                return Err(Issue::fail(format!(
                    "passes commit types before {arg}, so it fails on every commit"
                )));
            }
            if arg == "--" {
                options = false;
            } else if arg.len() >= "--sc".len() && "--scopes".starts_with(arg) {
                args.next();
            }
        } else {
            types.insert(arg);
        }
    }
    if types.is_empty() {
        return Err(Issue::warn(
            "passes no commit types, so it allows the hook's defaults, which a release may change",
        ));
    } else if CONVENTIONAL_TYPES
        .iter()
        .all(|commit_type| !types.contains(commit_type))
    {
        types.extend(CONVENTIONAL_TYPES);
    }
    Ok(types)
}

/// The module that `python -m` runs conventional-pre-commit as.
const CONVENTIONAL_PRE_COMMIT_MODULE: &str = "conventional_pre_commit.hook";

/// The arguments a command passes to conventional-pre-commit, if it runs it: as the module `python -m` runs, or as the
/// program, by name or path.
fn conventional_pre_commit_args<'a, 'b>(words: &'b [&'a str]) -> Option<&'b [&'a str]> {
    // The package's name may come first as the value of an option, as in `uvx --from conventional-pre-commit
    // conventional-pre-commit` or `uv run --with conventional-pre-commit python -m ...`, so the module, or the last
    // word naming the program, is the one run.
    if let Some(start) = words
        .windows(2)
        .position(|pair| pair == ["-m", CONVENTIONAL_PRE_COMMIT_MODULE])
    {
        return words.get(start + 2..);
    }
    let start = words
        .iter()
        .rposition(|word| program_name(word) == "conventional-pre-commit")?;
    words.get(start + 1..)
}

/// What is wrong with the commit types that conventional-pre-commit allows given the words after the program: nothing
/// if they are exactly the recognized ones.
fn conventional_types_issues(args: &[&str]) -> Vec<Issue> {
    let types = match conventional_commit_types(args) {
        Err(issue) => return vec![issue],
        Ok(types) => types,
    };
    let missing: Vec<_> = COMMIT_TYPES
        .into_iter()
        .filter(|commit_type| !types.contains(commit_type))
        .collect();
    let extra: Vec<_> = types
        .iter()
        .copied()
        .filter(|commit_type| !COMMIT_TYPES.contains(commit_type))
        .collect();
    let mut issues = Vec::new();
    if !missing.is_empty() {
        issues.push(Issue::warn(format!("doesn't allow {}", missing.join(", "))));
    }
    if !extra.is_empty() {
        issues.push(Issue::warn(format!("also allows {}", extra.join(", "))));
    }
    issues
}

/// Whether a conventional-pre-commit hook checks each commit message for the recognized types and no others. It must
/// run in the `commit-msg` stage, which the hook's own repository sets for it unless the configuration lists other
/// stages; a local hook, or one from another repository, lists it itself.
fn conventional_hook_in(config: &PrecommitConfig) -> Result<(), Issue> {
    let mut issues = Vec::new();

    for (repo, hook) in config
        .repos
        .iter()
        .flat_map(|repo| repo.hooks.iter().map(move |hook| (repo, hook)))
        .filter(|(_, hook)| hook.id == "conventional-pre-commit")
    {
        let mut hook_issues = Vec::new();

        let commit_msg = hook.stages.as_ref().map_or_else(
            || repo.repo.contains(CONVENTIONAL_PRE_COMMIT_REPO),
            |stages| stages.iter().any(|stage| stage == "commit-msg"),
        );
        // In any other stage, it never sees a commit message: pre-commit passes it the staged files instead, or, in
        // the manual stage, doesn't run it at all.
        if !commit_msg {
            hook_issues.push(Issue::fail("doesn't run in the commit-msg stage"));
        }

        let runs = hook.filter_map_commands("conventional-pre-commit", |words| {
            Some(conventional_types_issues(conventional_pre_commit_args(
                words,
            )?))
        });
        if runs.is_empty() {
            hook_issues.push(Issue::fail("doesn't run conventional-pre-commit"));
        } else if !runs.iter().any(Vec::is_empty) {
            hook_issues.extend(runs.into_iter().flatten());
        }

        let Some(severity) = hook_issues.iter().map(|issue| issue.severity).max() else {
            return Ok(());
        };
        let messages: Vec<_> = hook_issues.into_iter().map(|issue| issue.message).collect();
        issues.push(Issue {
            severity,
            message: format!("conventional-pre-commit {}", messages.join(" and ")),
        });
    }

    if issues.is_empty() {
        Err(Issue::fail("No conventional-pre-commit hook"))
    } else {
        issues.dedup();
        Issue::alternatives(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conventional(repo: &str, hook: &str) -> Result<(), Issue> {
        conventional_hook_in(
            &serde_saphyr::from_str(&format!(
                "repos:\n  - repo: {repo}\n    hooks:\n      - {hook}\n"
            ))
            .unwrap(),
        )
    }

    const CONVENTIONAL_REPO: &str = "https://github.com/compilerla/conventional-pre-commit";

    #[test]
    fn conventional_pre_commit_runs_as_the_last_program_word_or_module() {
        for entry in [
            "uv run --with conventional-pre-commit python -m conventional_pre_commit.hook",
            "uvx --from conventional-pre-commit conventional-pre-commit",
        ] {
            let hook = format!(
                "{{id: conventional-pre-commit, entry: {entry} feat fix perf refactor style test docs build ci revert \
                 chore, language: unsupported, stages: [commit-msg]}}"
            );
            assert_eq!(conventional("local", &hook), Ok(()), "{entry}");
        }
    }

    #[test]
    fn conventional_hook_passes_with_commit_msg_and_the_recognized_types() {
        for (repo, hook) in [
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [commit-msg], args: [feat, fix, perf, refactor, style, test, docs, \
                 build, ci, revert, chore]}",
            ),
            // The hook's repository sets the stage.
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, \
                 chore]}",
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: python -m conventional_pre_commit.hook feat fix perf refactor \
                 style test docs build ci revert chore, language: python, stages: [commit-msg]}",
            ),
            // feat and fix are added to types that name neither.
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--st, --strict, --scopes, api, perf, refactor, style, test, docs, \
                 build, ci, revert, chore]}",
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: .venv/bin/conventional-pre-commit --strict, language: unsupported, \
                 stages: [commit-msg, prepare-commit-msg], args: [feat, fix, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
            ),
        ] {
            assert_eq!(conventional(repo, hook), Ok(()), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_fails_when_absent_or_broken_and_warns_otherwise() {
        for (repo, hook, issue) in [
            (
                CONVENTIONAL_REPO,
                "{id: commitizen}",
                Issue::fail("No conventional-pre-commit hook"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [pre-commit]}",
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and passes no commit types, so it allows \
                 the hook's defaults, which a release may change",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--strict]}",
                Issue::warn(
                    "conventional-pre-commit passes no commit types, so it allows the hook's defaults, which a release may \
                 change",
                ),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: python -m conventional_pre_commit feat, stages: [commit-msg]}",
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: conventional-pre-commit feat, language: unsupported}",
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and doesn't allow fix, perf, refactor, \
                 style, test, docs, build, ci, revert, chore",
                ),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: commitlint --edit, stages: [commit-msg]}",
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--sc, chore, --, feat, fix, wip, --docs]}",
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, docs, build, ci, revert, chore and \
                 also allows --docs, wip",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [feat, fix, --strict, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
                Issue::fail(
                    "conventional-pre-commit passes commit types before --strict, so it fails on every commit",
                ),
            ),
            // Only the hook's own repository sets the stage, and an empty list falls back to default_stages.
            (
                "https://github.com/example/mirror",
                "{id: conventional-pre-commit, args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, \
                 chore]}",
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [], args: [feat, fix, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [commit-msg], args: [--scopes=api, docs]}",
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, build, ci, revert, chore",
                ),
            ),
        ] {
            assert_eq!(conventional(repo, hook), Err(issue), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_is_judged_by_its_best_hook() {
        let config = serde_saphyr::from_str(&format!(
            "repos:\n  - repo: {CONVENTIONAL_REPO}\n    hooks:\n      - {{id: conventional-pre-commit, args: [feat, fix]}}\n\
             \x20 - repo: local\n    hooks:\n      - {{id: conventional-pre-commit, alias: lint, entry: commitlint \
             --edit, stages: [commit-msg]}}\n"
        ))
        .unwrap();
        assert_eq!(
            conventional_hook_in(&config),
            Err(Issue::warn(
                "conventional-pre-commit doesn't allow perf, refactor, style, test, docs, build, ci, revert, chore; \
                 conventional-pre-commit doesn't run conventional-pre-commit"
            ))
        );
    }

    #[test]
    fn pre_commit_hooks_fail_when_missing_and_warn_in_another_stage() {
        let config = |hooks: &str| -> PrecommitConfig {
            serde_saphyr::from_str(&format!(
                "repos:\n  - repo: https://github.com/pre-commit/pre-commit-hooks\n    hooks: [{hooks}]\n"
            ))
            .unwrap()
        };
        let all: Vec<_> = PRE_COMMIT_HOOKS
            .iter()
            .map(|id| format!("{{id: {id}}}"))
            .collect();
        assert_eq!(pre_commit_hooks_in(&config(&all.join(", "))), Ok(()));

        let in_stage = |stage: &str| {
            all.iter()
                .map(|hook| {
                    hook.replace(
                        "{id: check-yaml}",
                        &format!("{{id: check-yaml, stages: [{stage}]}}"),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let pre_push = in_stage("pre-push");
        assert_eq!(
            pre_commit_hooks_in(&config(&pre_push)),
            Err(Issue::warn(
                "The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );
        assert_eq!(
            pre_commit_hooks_in(&config(&in_stage("manual"))),
            Err(Issue::fail(
                "The check-yaml hook runs only in the manual stage, so it never runs on its own"
            ))
        );

        let without_toml: Vec<_> = pre_push
            .split(", ")
            .filter(|hook| *hook != "{id: check-toml}")
            .collect();
        assert_eq!(
            pre_commit_hooks_in(&config(&without_toml.join(", "))),
            Err(Issue::fail(
                "Missing: check-toml; The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );
    }

    #[test]
    fn zizmor_hooks_need_the_pedantic_flag() {
        let hook = |entry: Option<&str>, args: &[&str]| PrecommitHook {
            id: "zizmor".to_owned(),
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrecommitHook::default()
        };
        assert!(zizmor_hook_pedantic(&hook(None, &["--pedantic"])));
        assert!(zizmor_hook_pedantic(&hook(
            None,
            &["--offline", "--pedantic"]
        )));
        assert!(zizmor_hook_pedantic(&hook(Some("zizmor --pedantic"), &[])));
        assert!(zizmor_hook_pedantic(&hook(
            Some("uvx zizmor"),
            &["--pedantic"]
        )));
        assert!(!zizmor_hook_pedantic(&hook(None, &[])));
        assert!(!zizmor_hook_pedantic(&hook(
            Some("zizmor --offline"),
            &["."]
        )));
        assert!(!zizmor_hook_pedantic(&hook(Some("echo --pedantic"), &[])));
        assert!(!zizmor_hook_pedantic(&hook(None, &["--pedantic-ish"])));
        // Other spellings of the persona, and the stricter auditor one, aren't the standard's.
        for args in [
            &["-p"][..],
            &["--persona=pedantic"],
            &["--persona", "pedantic"],
            &["--persona=auditor"],
        ] {
            assert!(!zizmor_hook_pedantic(&hook(None, args)), "{args:?}");
        }
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of `prek.toml` that apply to every project: its settings and the common hooks, including
//! the commit message hook.

use std::collections::BTreeSet;

use super::ci::zizmor_command_pedantic;
use crate::config::{FormatterRun, PrekConfig, PrekHook, PrekRepo, is_program, program_name};
use crate::context::ProjectContext;
use crate::report::{Issue, Report};

//
// prek
//

/// The repository of hooks built into prek, which it runs without cloning or installing anything.
const BUILTIN_REPO: &str = "builtin";

/// The hooks from prek's builtin repository that every project runs.
const COMMON_HOOKS: [&str; 11] = [
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

/// Whether each of the common hooks runs before each commit, from prek's builtin repository. One from another, such as
/// pre-commit-hooks, which prek clones and installs Python for, still checks the files, so it only warns.
fn common_hooks_in(config: &PrekConfig) -> Result<(), Issue> {
    let mut missing = Vec::new();
    let mut elsewhere = Vec::new();
    let mut issues = Vec::new();
    for id in COMMON_HOOKS {
        let mut repos = config
            .repos
            .iter()
            .filter(|repo| repo.hooks.iter().any(|hook| hook.id == id))
            .peekable();
        if repos.peek().is_none() {
            missing.push(id);
            continue;
        }
        if !repos.any(|repo| repo.repo == BUILTIN_REPO) {
            elsewhere.push(id);
        }
        if let Err(issue) = config.check_commit_hook(id) {
            issues.push(issue);
        }
    }
    if !elsewhere.is_empty() {
        issues.insert(
            0,
            Issue::warn(format!(
                "Not from prek's builtin repo: {}",
                elsewhere.join(", ")
            )),
        );
    }
    if !missing.is_empty() {
        issues.insert(0, Issue::fail(format!("Missing: {}", missing.join(", "))));
    }
    Issue::combine(issues)
}

/// Whether a zizmor hook runs the pedantic persona, passing `--pedantic` in its entry or the args prek appends to it.
/// A hook without an entry, as a remote hook may leave it to the hook's repository, is taken to run `zizmor`.
fn zizmor_hook_pedantic(hook: &PrekHook) -> bool {
    hook.filter_map_commands("zizmor", zizmor_command_pedantic)
        .contains(&true)
}

pub(crate) fn check_prek(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("prek-config", "prek.toml", ctx.prek()) else {
        return;
    };

    // Without both types, prek install leaves the pre-commit or the commit-msg hooks uninstalled, so they never run,
    // and prek rejects a list it can't read.
    report.must(
        "prek-install-hook-types",
        "prek: default_install_hook_types includes pre-commit and commit-msg",
        config.check_default_install_hook_types(),
    );
    // With any other list, hooks without stages of their own run in other stages than once before each commit, and
    // prek rejects a list it can't read, so it runs no hooks at all.
    report.check(
        "prek-default-stages",
        "prek: default_stages is [pre-commit]",
        config.check_default_stages(),
    );
    // Renovate would leave such a repo at its revision, which still runs.
    report.should(
        "prek-rev-follows-repo",
        "prek: each remote repo's rev follows it, where Renovate finds it",
        config.check_renovate_revs(),
    );
    report.should(
        "prek-hook-ids-unique",
        "prek: each hook has a unique ID or alias",
        config.check_unique_hook_ids(),
    );
    report.check(
        "prek-builtin-hooks",
        "prek: the common hooks from prek's builtin repo",
        common_hooks_in(config),
    );
    report.check(
        "prek-reuse",
        "prek: reuse hook",
        config.check_commit_hook("reuse"),
    );
    report.check(
        "prek-markdownlint",
        "prek: markdownlint-cli2 hook",
        config.check_commit_hook("markdownlint-cli2"),
    );
    report.check(
        "prek-tombi",
        "prek: tombi-format hook (with --check)",
        config.check_formatter_hook("tombi-format", "tombi format", tombi_format_run),
    );
    report.check(
        "prek-conventional-pre-commit",
        "prek: conventional-pre-commit hook (commit-msg stage, recognized types)",
        conventional_hook_in(config),
    );
    report.check(
        "prek-norms-commit-msg",
        "prek: norms-commit-msg hook (commit-msg stage)",
        norms_commit_msg_hook_in(config),
    );
    report.check(
        "prek-norms-verify",
        "prek: norms-verify hook (--deny-warnings)",
        norms_verify_hook_in(config),
    );
    report.check(
        "prek-zizmor",
        "prek: zizmor hook (pedantic)",
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
// norms
//

/// The repository of norms, whose hooks set their stages themselves.
const NORMS_REPO: &str = "github.com/aexoden/norms";

/// Whether `hook`, from `repo`, runs on commit messages, in the `commit-msg` stage: by the stages it lists, or, if it
/// lists none, by `repo` being `hook_repo`, whose manifest sets that stage for the hook, which `default_stages` doesn't
/// override. A local hook, or one from another repository, lists the stage itself.
fn runs_on_commit_messages(repo: &PrekRepo, hook: &PrekHook, hook_repo: &str) -> bool {
    hook.stages.as_ref().map_or_else(
        || repo.repo.contains(hook_repo),
        |stages| stages.iter().any(|stage| stage == "commit-msg"),
    )
}

/// Whether a norms-commit-msg hook checks each commit message: it runs in the `commit-msg` stage, which the norms
/// repository's hook sets for it unless the configuration lists other stages, and a local hook lists itself. In any
/// other stage, it never sees a commit message.
fn norms_commit_msg_hook_in(config: &PrekConfig) -> Result<(), Issue> {
    let mut found = false;
    for (repo, hook) in config
        .repos
        .iter()
        .flat_map(|repo| repo.hooks.iter().map(move |hook| (repo, hook)))
        .filter(|(_, hook)| hook.id == "norms-commit-msg")
    {
        found = true;
        if runs_on_commit_messages(repo, hook, NORMS_REPO) {
            return Ok(());
        }
    }
    Err(Issue::fail(if found {
        "The norms-commit-msg hook doesn't run in the commit-msg stage"
    } else {
        "No norms-commit-msg hook"
    }))
}

/// Whether a norms-verify hook runs before each commit with `--deny-warnings`, in its entry or the args prek appends
/// to it, as the standards require. A hook without an entry, as the norms repository's leaves it, runs
/// `norms verify .`.
fn norms_verify_hook_in(config: &PrekConfig) -> Result<(), Issue> {
    if config
        .commit_hooks("norms-verify")?
        .into_iter()
        .any(|hook| {
            hook.filter_map_commands("norms verify .", |words| {
                Some(words.contains(&"--deny-warnings"))
            })
            .contains(&true)
        })
    {
        Ok(())
    } else {
        Err(Issue::warn(
            "The norms-verify hook doesn't pass --deny-warnings, so it passes a project that warns",
        ))
    }
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
/// prek appends the commit message file, which argparse takes from the first run of positional arguments, so
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
fn conventional_hook_in(config: &PrekConfig) -> Result<(), Issue> {
    let mut issues = Vec::new();

    for (repo, hook) in config
        .repos
        .iter()
        .flat_map(|repo| repo.hooks.iter().map(move |hook| (repo, hook)))
        .filter(|(_, hook)| hook.id == "conventional-pre-commit")
    {
        let mut hook_issues = Vec::new();

        let commit_msg = runs_on_commit_messages(repo, hook, CONVENTIONAL_PRE_COMMIT_REPO);
        // In any other stage, it never sees a commit message: prek passes it the staged files instead, or, in
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

/// How a command's words run `tombi format`, if they do, after `tombi` anywhere among them, as in
/// `uvx tombi format`: the tombi-format hook's own entry runs it, and it checks only with `--check`.
fn tombi_format_run(words: &[&str]) -> Option<FormatterRun> {
    words
        .iter()
        .enumerate()
        .find_map(|(index, word)| match words.get(index + 1..)? {
            ["format", args @ ..] if is_program(word, "tombi") => {
                Some(FormatterRun::from_args(args, &["--check"], &[]))
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conventional(repo: &str, hook: &str) -> Result<(), Issue> {
        conventional_hook_in(
            &toml::from_str(&format!(
                "repos = [{{repo = \"{repo}\", hooks = [{hook}]}}]\n"
            ))
            .unwrap(),
        )
    }

    const CONVENTIONAL_REPO: &str = "https://github.com/compilerla/conventional-pre-commit";

    /// The recognized commit types, as a TOML list.
    const TYPES: &str = r#"["feat", "fix", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore"]"#;

    #[test]
    fn tombi_format_checks_only_with_the_check_flag() {
        let run = |line: &str| tombi_format_run(&line.split_whitespace().collect::<Vec<_>>());
        assert_eq!(run("tombi format --check"), Some(FormatterRun::Checks));
        assert_eq!(run("tombi format"), Some(FormatterRun::Rewrites));
        assert_eq!(run("uvx tombi format --check"), Some(FormatterRun::Checks));
        assert_eq!(run("tombi lint"), None);
    }

    #[test]
    fn conventional_pre_commit_runs_as_the_last_program_word_or_module() {
        for entry in [
            "uv run --with conventional-pre-commit python -m conventional_pre_commit.hook",
            "uvx --from conventional-pre-commit conventional-pre-commit",
        ] {
            let hook = format!(
                r#"{{id = "conventional-pre-commit", entry = "{entry} feat fix perf refactor style test docs build ci revert chore", language = "system", stages = ["commit-msg"]}}"#
            );
            assert_eq!(conventional("local", &hook), Ok(()), "{entry}");
        }
    }

    #[test]
    fn conventional_hook_passes_with_commit_msg_and_the_recognized_types() {
        for (repo, hook) in [
            (
                CONVENTIONAL_REPO,
                format!(
                    r#"{{id = "conventional-pre-commit", stages = ["commit-msg"], args = {TYPES}}}"#
                ),
            ),
            // The hook's repository sets the stage.
            (
                CONVENTIONAL_REPO,
                format!(r#"{{id = "conventional-pre-commit", args = {TYPES}}}"#),
            ),
            (
                "local",
                r#"{id = "conventional-pre-commit", entry = "python -m conventional_pre_commit.hook feat fix perf refactor style test docs build ci revert chore", language = "python", stages = ["commit-msg"]}"#
                    .to_owned(),
            ),
            // feat and fix are added to types that name neither.
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", args = ["--st", "--strict", "--scopes", "api", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore"]}"#
                    .to_owned(),
            ),
            (
                "local",
                format!(
                    r#"{{id = "conventional-pre-commit", entry = ".venv/bin/conventional-pre-commit --strict", language = "system", stages = ["commit-msg", "prepare-commit-msg"], args = {TYPES}}}"#
                ),
            ),
        ] {
            assert_eq!(conventional(repo, &hook), Ok(()), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_fails_when_absent_or_broken_and_warns_otherwise() {
        for (repo, hook, issue) in [
            (
                CONVENTIONAL_REPO,
                r#"{id = "commitizen"}"#.to_owned(),
                Issue::fail("No conventional-pre-commit hook"),
            ),
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", stages = ["pre-commit"]}"#.to_owned(),
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and passes no commit types, so it allows \
                 the hook's defaults, which a release may change",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", args = ["--strict"]}"#.to_owned(),
                Issue::warn(
                    "conventional-pre-commit passes no commit types, so it allows the hook's defaults, which a release may \
                 change",
                ),
            ),
            (
                "local",
                r#"{id = "conventional-pre-commit", entry = "python -m conventional_pre_commit feat", stages = ["commit-msg"]}"#
                    .to_owned(),
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                "local",
                r#"{id = "conventional-pre-commit", entry = "conventional-pre-commit feat", language = "system"}"#
                    .to_owned(),
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and doesn't allow fix, perf, refactor, \
                 style, test, docs, build, ci, revert, chore",
                ),
            ),
            (
                "local",
                r#"{id = "conventional-pre-commit", entry = "commitlint --edit", stages = ["commit-msg"]}"#.to_owned(),
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", args = ["--sc", "chore", "--", "feat", "fix", "wip", "--docs"]}"#
                    .to_owned(),
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, docs, build, ci, revert, chore and \
                 also allows --docs, wip",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", args = ["feat", "fix", "--strict", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore"]}"#
                    .to_owned(),
                Issue::fail(
                    "conventional-pre-commit passes commit types before --strict, so it fails on every commit",
                ),
            ),
            // Only the hook's own repository sets the stage, and an empty list falls back to default_stages.
            (
                "https://github.com/example/mirror",
                format!(r#"{{id = "conventional-pre-commit", args = {TYPES}}}"#),
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                format!(r#"{{id = "conventional-pre-commit", stages = [], args = {TYPES}}}"#),
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                r#"{id = "conventional-pre-commit", stages = ["commit-msg"], args = ["--scopes=api", "docs"]}"#
                    .to_owned(),
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, build, ci, revert, chore",
                ),
            ),
        ] {
            assert_eq!(conventional(repo, &hook), Err(issue), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_is_judged_by_its_best_hook() {
        let config = toml::from_str(&format!(
            r#"repos = [
  {{repo = "{CONVENTIONAL_REPO}", hooks = [{{id = "conventional-pre-commit", args = ["feat", "fix"]}}]}},
  {{repo = "local", hooks = [{{id = "conventional-pre-commit", alias = "lint", entry = "commitlint --edit", stages = ["commit-msg"]}}]}},
]
"#
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
    fn common_hooks_fail_when_missing_and_warn_in_another_stage_or_repo() {
        let config = |repo: &str, hooks: &str| -> PrekConfig {
            toml::from_str(&format!(
                "repos = [{{repo = \"{repo}\", hooks = [{hooks}]}}]\n"
            ))
            .unwrap()
        };
        let all: Vec<_> = COMMON_HOOKS
            .iter()
            .map(|id| format!("{{id = \"{id}\"}}"))
            .collect();
        assert_eq!(common_hooks_in(&config("builtin", &all.join(", "))), Ok(()));

        let in_stage = |stage: &str| {
            all.iter()
                .map(|hook| {
                    hook.replace(
                        r#"{id = "check-yaml"}"#,
                        &format!(r#"{{id = "check-yaml", stages = ["{stage}"]}}"#),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let pre_push = in_stage("pre-push");
        assert_eq!(
            common_hooks_in(&config("builtin", &pre_push)),
            Err(Issue::warn(
                "The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );
        assert_eq!(
            common_hooks_in(&config("builtin", &in_stage("manual"))),
            Err(Issue::fail(
                "The check-yaml hook runs only in the manual stage, so it never runs on its own"
            ))
        );

        let without_toml: Vec<_> = pre_push
            .split(", ")
            .filter(|hook| *hook != r#"{id = "check-toml"}"#)
            .collect();
        assert_eq!(
            common_hooks_in(&config("builtin", &without_toml.join(", "))),
            Err(Issue::fail(
                "Missing: check-toml; The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );

        // pre-commit-hooks has the same hooks, which prek clones and installs Python for.
        let remote = "https://github.com/pre-commit/pre-commit-hooks";
        assert_eq!(
            common_hooks_in(&config(remote, &all.join(", "))),
            Err(Issue::warn(format!(
                "Not from prek's builtin repo: {}",
                COMMON_HOOKS.join(", ")
            )))
        );
        let split: PrekConfig = toml::from_str(&format!(
            "repos = [{{repo = \"builtin\", hooks = [{}]}}, {{repo = \"{remote}\", hooks = [{{id = \"check-yaml\"}}]}}]\n",
            all.iter()
                .filter(|hook| !hook.contains("check-yaml"))
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .unwrap();
        assert_eq!(
            common_hooks_in(&split),
            Err(Issue::warn("Not from prek's builtin repo: check-yaml"))
        );
        // A hook in both, as during a move to the builtin repo, is one from it.
        let both: PrekConfig = toml::from_str(&format!(
            "repos = [{{repo = \"builtin\", hooks = [{}]}}, {{repo = \"{remote}\", hooks = [{{id = \"check-yaml\"}}]}}]\n",
            all.join(", ")
        ))
        .unwrap();
        assert_eq!(common_hooks_in(&both), Ok(()));
    }

    /// A configuration with `default_stages` and a repository, given as its URL, or `local`, and its hooks, an inline
    /// array of tables.
    fn config(repo: &str, hooks: &str) -> PrekConfig {
        toml::from_str(&format!(
            "default_stages = [\"pre-commit\"]\nrepos = [{{repo = \"{repo}\", rev = \"v0.2.0\", hooks = {hooks}}}]\n"
        ))
        .unwrap()
    }

    const NORMS: &str = "https://github.com/aexoden/norms";

    #[test]
    fn norms_commit_msg_hook_runs_in_the_commit_msg_stage() {
        // The norms repository's hook sets the stage, which default_stages doesn't override.
        assert_eq!(
            norms_commit_msg_hook_in(&config(NORMS, r#"[{id = "norms-commit-msg"}]"#)),
            Ok(())
        );
        assert_eq!(
            norms_commit_msg_hook_in(&config(
                "local",
                r#"[{id = "norms-commit-msg", entry = "cargo run -- commit-msg", language = "system", stages = ["commit-msg"]}]"#
            )),
            Ok(())
        );
        // A local hook without stages gets default_stages.
        assert_eq!(
            norms_commit_msg_hook_in(&config(
                "local",
                r#"[{id = "norms-commit-msg", entry = "norms commit-msg", language = "system"}]"#
            )),
            Err(Issue::fail(
                "The norms-commit-msg hook doesn't run in the commit-msg stage"
            ))
        );
        assert_eq!(
            norms_commit_msg_hook_in(&config(
                NORMS,
                r#"[{id = "norms-commit-msg", stages = ["pre-commit"]}]"#
            )),
            Err(Issue::fail(
                "The norms-commit-msg hook doesn't run in the commit-msg stage"
            ))
        );
        assert_eq!(
            norms_commit_msg_hook_in(&config(NORMS, r#"[{id = "norms-verify"}]"#)),
            Err(Issue::fail("No norms-commit-msg hook"))
        );
    }

    #[test]
    fn norms_verify_hook_denies_warnings() {
        assert_eq!(
            norms_verify_hook_in(&config(
                NORMS,
                r#"[{id = "norms-verify", args = ["--deny-warnings"]}]"#
            )),
            Ok(())
        );
        assert_eq!(
            norms_verify_hook_in(&config(
                "local",
                r#"[{id = "norms-verify", entry = "cargo run -- verify --deny-warnings .", language = "system", pass_filenames = false}]"#
            )),
            Ok(())
        );
        assert_eq!(
            norms_verify_hook_in(&config(NORMS, r#"[{id = "norms-verify"}]"#)),
            Err(Issue::warn(
                "The norms-verify hook doesn't pass --deny-warnings, so it passes a project that warns"
            ))
        );
        assert_eq!(
            norms_verify_hook_in(&config(
                NORMS,
                r#"[{id = "norms-verify", args = ["--deny-warnings"], stages = ["manual"]}]"#
            )),
            Err(Issue::fail(
                "The norms-verify hook runs only in the manual stage, so it never runs on its own"
            ))
        );
        assert_eq!(
            norms_verify_hook_in(&config(NORMS, r#"[{id = "norms-commit-msg"}]"#)),
            Err(Issue::fail("No norms-verify hook"))
        );
    }

    #[test]
    fn zizmor_hooks_need_the_pedantic_flag() {
        let hook = |entry: Option<&str>, args: &[&str]| PrekHook {
            id: "zizmor".to_owned(),
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrekHook::default()
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

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `'.pre-commit-config.yaml`.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::report::Issue;

/// The hook types pre-commit can install, from its `clientlib.HOOK_TYPES`.
const HOOK_TYPES: [&str; 10] = [
    "commit-msg",
    "post-checkout",
    "post-commit",
    "post-merge",
    "post-rewrite",
    "pre-commit",
    "pre-merge-commit",
    "pre-push",
    "pre-rebase",
    "prepare-commit-msg",
];

/// Top-level `.pre-commit-config.yaml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrecommitConfig {
    /// The hook types a plain `pre-commit install` installs, kept as YAML wrote it, so that a malformed value fails
    /// only the check reading it. A null value is `Some(Value::Null)`, which pre-commit rejects, unlike a missing one.
    #[serde(deserialize_with = "present")]
    pub default_install_hook_types: Option<Value>,
    /// The stages a hook runs in when it doesn't list its own, kept as YAML wrote it, so that a malformed value fails
    /// only the check reading it. A null value is `Some(Value::Null)`, which pre-commit rejects, unlike a missing one.
    #[serde(deserialize_with = "present")]
    pub default_stages: Option<Value>,
    pub repos: Vec<PrecommitRepo>,
}

/// A value that is present, even if null, so that it can be told apart from a missing one, which `#[serde(default)]`
/// leaves `None`.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// A value that is present and not null, which pre-commit requires of a key it validates whenever it is given, though
/// `Option` alone would read a null value as a missing one.
fn not_null<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// A single repo entry in the pre-commit config.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrecommitRepo {
    /// The repository URL (or `"local"` for local hooks).
    pub repo: String,
    /// The revision/tag to use.
    pub rev: Option<String>,
    /// Hooks defined in this repo.
    pub hooks: Vec<PrecommitHook>,
}

/// A single hook within a pre-commit repo.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrecommitHook {
    pub id: String,
    /// Another name that `SKIP` and `pre-commit run` match; empty or null means none.
    pub alias: Option<String>,
    pub name: Option<String>,
    pub entry: Option<String>,
    pub language: Option<String>,
    /// The stages the hook lists, if it lists any. A null value fails parsing, as pre-commit rejects it, rather than
    /// reading as no list.
    #[serde(deserialize_with = "not_null")]
    pub stages: Option<Vec<String>>,
    pub args: Vec<String>,
}

impl PrecommitHook {
    /// The hook's alias, if it has one other than its ID.
    pub(crate) fn alias(&self) -> Option<&str> {
        self.alias
            .as_deref()
            .filter(|alias| !alias.is_empty() && *alias != self.id)
    }

    /// The command the hook runs, as its words, in a list of one, or of none if the entry has unbalanced quotes.
    /// pre-commit splits the hook's entry, or `default_entry` if it sets none, as a remote hook may leave it to the
    /// hook's repository, with Python's `shlex`, and appends the args. The `shlex` crate splits it the same way, except
    /// that it reads a word starting with `#` as a comment ending the entry, which YAML has usually removed already.
    pub(crate) fn commands(&self, default_entry: &str) -> Vec<Vec<String>> {
        let Some(mut words) = shlex::split(self.entry.as_deref().unwrap_or(default_entry)) else {
            return Vec::new();
        };
        words.extend(self.args.iter().cloned());
        vec![words]
    }

    /// The results of `f` for each command the hook runs, as [`commands`](Self::commands) gives them, that it gives one
    /// for.
    pub(crate) fn filter_map_commands<T>(
        &self,
        default_entry: &str,
        mut f: impl FnMut(&[&str]) -> Option<T>,
    ) -> Vec<T> {
        self.commands(default_entry)
            .iter()
            .filter_map(|command| f(&command.iter().map(String::as_str).collect::<Vec<_>>()))
            .collect()
    }
}

impl PrecommitConfig {
    /// Whether `default_stages` is exactly `[pre-commit]`, so that hooks without stages of their own run once per
    /// commit, and not again on its message: a failure if pre-commit would reject it, as anything but a list of the
    /// stages it knows, and a warning if it is missing or lists other stages.
    pub(crate) fn check_default_stages(&self) -> Result<(), Issue> {
        match &self.default_stages {
            None => Err(Issue::warn(
                "No default_stages, so hooks without stages of their own run in every installed stage",
            )),
            Some(Value::Array(stages)) => {
                let unknown: Vec<_> = stages
                    .iter()
                    .filter(|stage| !stage.as_str().is_some_and(is_stage))
                    .map(value_name)
                    .collect();
                if !unknown.is_empty() {
                    Err(Issue::fail(format!(
                        "default_stages lists {}, which pre-commit doesn't know",
                        unknown.join(", ")
                    )))
                } else if matches!(stages.as_slice(), [stage] if stage == "pre-commit") {
                    Ok(())
                } else {
                    Err(Issue::warn(format!(
                        "default_stages is [{}], not [pre-commit]",
                        stages.iter().map(value_name).collect::<Vec<_>>().join(", ")
                    )))
                }
            }
            Some(_) => Err(Issue::fail("default_stages isn't a list")),
        }
    }

    /// Whether `default_install_hook_types` lists both `pre-commit` and `commit-msg`, so that a plain `pre-commit
    /// install` also installs the commit message hook. Any other hook types pre-commit knows may be listed too.
    pub(crate) fn check_default_install_hook_types(&self) -> Result<(), String> {
        const REQUIRED: [&str; 2] = ["pre-commit", "commit-msg"];
        match &self.default_install_hook_types {
            None => Err(
                "No default_install_hook_types, so pre-commit install installs only the pre-commit hook".to_owned(),
            ),
            Some(Value::Array(types)) => {
                let missing: Vec<_> = REQUIRED
                    .into_iter()
                    .filter(|required| !types.iter().any(|hook_type| hook_type == required))
                    .collect();
                let unknown: Vec<_> = types
                    .iter()
                    .filter(|hook_type| !hook_type.as_str().is_some_and(|name| HOOK_TYPES.contains(&name)))
                    .map(value_name)
                    .collect();
                if !unknown.is_empty() {
                    Err(format!(
                        "default_install_hook_types lists {}, which pre-commit doesn't know",
                        unknown.join(", ")
                    ))
                } else if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!("default_install_hook_types lacks {}", missing.join(" and ")))
                }
            }
            Some(_) => Err("default_install_hook_types isn't a list".to_owned()),
        }
    }

    /// Whether `SKIP` (or `pre-commit run`) can select each hook alone: each hook needs an ID or alias that no other
    /// hook has as its ID or alias. pre-commit matches a hook by either, so the same remote hook can run twice under
    /// one ID if each copy has an alias of its own.
    pub(crate) fn check_unique_hook_ids(&self) -> Result<(), String> {
        fn names(hook: &PrecommitHook) -> Vec<&str> {
            let mut names = vec![hook.id.as_str()];
            names.extend(hook.alias());
            names
        }

        let hooks: Vec<_> = self.repos.iter().flat_map(|repo| &repo.hooks).collect();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for hook in &hooks {
            for name in names(hook) {
                *counts.entry(name).or_default() += 1;
            }
        }
        let mut ambiguous: Vec<&str> = Vec::new();
        for hook in &hooks {
            let label = if hook.id.is_empty() {
                "(no id)"
            } else {
                &hook.id
            };
            if names(hook)
                .iter()
                .all(|name| counts.get(name).is_some_and(|&count| count > 1))
                && !ambiguous.contains(&label)
            {
                ambiguous.push(label);
            }
        }
        if ambiguous.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Hooks with no ID or alias of their own: {}",
                ambiguous.join(", ")
            ))
        }
    }

    /// The stages `hook` runs in, by the names the configuration gives them: those it lists, or, if it lists none or an
    /// empty list, `default_stages`. Without either, pre-commit runs it in every stage its repository gives it, which
    /// is none. A malformed `default_stages` is left to [`check_default_stages`](Self::check_default_stages).
    fn stages_of<'a>(&'a self, hook: &'a PrecommitHook) -> Option<Vec<&'a str>> {
        match (&hook.stages, &self.default_stages) {
            (Some(stages), _) if !stages.is_empty() => {
                Some(stages.iter().map(String::as_str).collect())
            }
            (_, Some(Value::Array(stages))) => {
                Some(stages.iter().filter_map(Value::as_str).collect())
            }
            (_, _) => None,
        }
    }

    /// Whether `hook` runs in `stage`, as [`stages_of`](Self::stages_of) reads its stages. One that runs in every stage
    /// its repository gives it is taken to run in `stage`, which for the hooks checked is among them. Stages may be
    /// given by their old names, such as `commit`.
    fn runs_in(&self, hook: &PrecommitHook, stage: &str) -> bool {
        let wanted = current_stage_name(stage);
        self.stages_of(hook).is_none_or(|stages| {
            stages
                .into_iter()
                .any(|listed| current_stage_name(listed) == wanted)
        })
    }

    /// Whether `hook` runs only in the `manual` stage, as [`stages_of`](Self::stages_of) reads its stages, so that it
    /// runs only when `pre-commit run --hook-stage manual` asks for it.
    fn runs_only_manually(&self, hook: &PrecommitHook) -> bool {
        self.stages_of(hook).is_some_and(|stages| {
            !stages.is_empty() && stages.into_iter().all(|stage| stage == "manual")
        })
    }

    /// Whether `hook` runs before each commit, in the `pre-commit` stage. See [`runs_in`](Self::runs_in).
    fn runs_at_commit(&self, hook: &PrecommitHook) -> bool {
        self.runs_in(hook, "pre-commit")
    }

    /// Every hook that runs in `stage`, as [`runs_in`](Self::runs_in) decides.
    pub(crate) fn hooks_in<'a>(
        &'a self,
        stage: &'a str,
    ) -> impl Iterator<Item = &'a PrecommitHook> {
        self.repos
            .iter()
            .flat_map(|repo| &repo.hooks)
            .filter(move |hook| self.runs_in(hook, stage))
    }

    /// The hooks with ID `id` that run before each commit, or, if there are none, why not: a failure if there is no
    /// such hook or it runs only in the `manual` stage, so never on its own, and a warning if it runs only in other
    /// stages.
    pub(crate) fn commit_hooks(&self, id: &str) -> Result<Vec<&PrecommitHook>, Issue> {
        let hooks: Vec<_> = self
            .repos
            .iter()
            .flat_map(|repo| &repo.hooks)
            .filter(|hook| hook.id == id)
            .collect();
        if hooks.is_empty() {
            return Err(Issue::fail(format!("No {id} hook")));
        }
        if hooks.iter().all(|hook| self.runs_only_manually(hook)) {
            return Err(Issue::fail(format!(
                "The {id} hook runs only in the manual stage, so it never runs on its own"
            )));
        }

        let hooks: Vec<_> = hooks
            .into_iter()
            .filter(|hook| self.runs_at_commit(hook))
            .collect();
        if hooks.is_empty() {
            Err(Issue::warn(format!(
                "The {id} hook doesn't run in the pre-commit stage"
            )))
        } else {
            Ok(hooks)
        }
    }

    /// Whether a hook with ID `id` runs before each commit. See [`commit_hooks`](Self::commit_hooks).
    pub(crate) fn check_commit_hook(&self, id: &str) -> Result<(), Issue> {
        self.commit_hooks(id).map(|_| ())
    }
}

/// A YAML value as a message names it: a string as itself, anything else as JSON writes it.
fn value_name(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Whether pre-commit knows `stage` as a stage, by its current or its old name: a hook type, or `manual`.
fn is_stage(stage: &str) -> bool {
    stage == "manual" || HOOK_TYPES.contains(&current_stage_name(stage))
}

/// The current name of a pre-commit stage, given by its current or its old name, which pre-commit still accepts.
fn current_stage_name(stage: &str) -> &str {
    match stage {
        "commit" => "pre-commit",
        "merge-commit" => "pre-merge-commit",
        "push" => "pre-push",
        stage => stage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_stages(yaml: &str) -> Result<(), Issue> {
        serde_saphyr::from_str::<PrecommitConfig>(yaml)
            .unwrap()
            .check_default_stages()
    }

    fn default_install_hook_types(yaml: &str) -> Result<(), String> {
        serde_saphyr::from_str::<PrecommitConfig>(yaml)
            .unwrap()
            .check_default_install_hook_types()
    }

    #[test]
    fn default_install_hook_types_must_include_commit_msg() {
        for yaml in [
            "default_install_hook_types: [pre-commit, commit-msg]\n",
            "default_install_hook_types: [commit-msg, pre-commit]\n",
            "default_install_hook_types: [pre-commit, commit-msg, pre-push]\n",
        ] {
            assert_eq!(default_install_hook_types(yaml), Ok(()), "{yaml}");
        }
        for (yaml, error) in [
            (
                "repos: []\n",
                "No default_install_hook_types, so pre-commit install installs only the pre-commit hook",
            ),
            (
                "default_install_hook_types:\n",
                "default_install_hook_types isn't a list",
            ),
            (
                "default_install_hook_types: ~\n",
                "default_install_hook_types isn't a list",
            ),
            (
                "default_install_hook_types: [pre-commit]\n",
                "default_install_hook_types lacks commit-msg",
            ),
            (
                "default_install_hook_types: [commit-msg, pre-push]\n",
                "default_install_hook_types lacks pre-commit",
            ),
            (
                "default_install_hook_types: []\n",
                "default_install_hook_types lacks pre-commit and commit-msg",
            ),
            (
                "default_install_hook_types: pre-commit\n",
                "default_install_hook_types isn't a list",
            ),
            (
                "default_install_hook_types: [pre-commit, commit-msg, pre_push, 3]\n",
                "default_install_hook_types lists pre_push, 3, which pre-commit doesn't know",
            ),
        ] {
            assert_eq!(
                default_install_hook_types(yaml),
                Err(error.to_owned()),
                "{yaml}"
            );
        }
    }

    fn unique_hook_ids(hooks: &str) -> Result<(), String> {
        serde_saphyr::from_str::<PrecommitConfig>(&format!(
            "repos:\n  - repo: local\n    hooks: {hooks}\n"
        ))
        .unwrap()
        .check_unique_hook_ids()
    }

    #[test]
    fn hooks_need_an_id_or_alias_of_their_own() {
        for hooks in [
            "[]",
            "[{id: pytest}, {id: cargo-test}, {id: vitest}]",
            "[{id: eslint, alias: eslint-admin}, {id: eslint, alias: eslint-web}]",
            "[{id: test, alias: test}, {id: vitest}]",
            "[{id: test, alias: ~}, {id: vitest}]",
            "[{id: test, alias: ''}, {id: vitest}]",
        ] {
            assert_eq!(unique_hook_ids(hooks), Ok(()), "{hooks}");
        }
        for (hooks, ambiguous) in [
            ("[{id: test}, {id: test}]", "test"),
            ("[{id: test}, {id: test}, {id: test}]", "test"),
            ("[{id: test}, {id: test, alias: test}]", "test"),
            // SKIP=eslint-admin selects the first hook alone, but nothing selects the second.
            (
                "[{id: eslint, alias: eslint-admin}, {id: eslint}]",
                "eslint",
            ),
            ("[{id: a}, {id: b}, {id: a}, {id: b}]", "a, b"),
            ("[{id: test, alias: ~}, {id: test}]", "test"),
            ("[{alias: a}, {alias: a}]", "(no id)"),
            (
                "[{id: eslint, alias: lint}, {id: eslint, alias: lint}]",
                "eslint",
            ),
            // The first hook's only name is the second's alias; the second has an ID of its own.
            ("[{id: vitest}, {id: test, alias: vitest}]", "vitest"),
        ] {
            assert_eq!(
                unique_hook_ids(hooks),
                Err(format!(
                    "Hooks with no ID or alias of their own: {ambiguous}"
                )),
                "{hooks}"
            );
        }
    }

    fn hook_commands(entry: Option<&str>, args: &[&str]) -> Vec<Vec<String>> {
        PrecommitHook {
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrecommitHook::default()
        }
        .commands("default")
    }

    #[test]
    fn entries_split_as_pre_commit_splits_them() {
        for (entry, args, expected) in [
            (None, &["--fix"][..], vec![vec!["default", "--fix"]]),
            (
                Some("pnpm exec eslint . --max-warnings 0"),
                &[],
                vec![vec!["pnpm", "exec", "eslint", ".", "--max-warnings", "0"]],
            ),
            (
                Some("python -c 'import sys; sys.exit(0)'"),
                &["a"],
                vec![vec!["python", "-c", "import sys; sys.exit(0)", "a"]],
            ),
            (
                Some("bash -c 'cd web && eslint .'"),
                &["--max-warnings", "0"],
                vec![vec![
                    "bash",
                    "-c",
                    "cd web && eslint .",
                    "--max-warnings",
                    "0",
                ]],
            ),
            (Some("eslint 'unbalanced"), &[], vec![]),
        ] {
            assert_eq!(hook_commands(entry, args), expected, "{entry:?}");
        }
    }

    fn commit_hook(yaml: &str) -> Result<(), Issue> {
        serde_saphyr::from_str::<PrecommitConfig>(yaml)
            .unwrap()
            .check_commit_hook("markdownlint-cli2")
    }

    #[test]
    fn commit_hooks_are_found_by_id() {
        assert_eq!(
            commit_hook(
                "repos:\n  - repo: local\n    hooks: [{id: markdownlint-cli2, entry: markdownlint}]\n"
            ),
            Ok(())
        );
        assert_eq!(
            commit_hook("repos: []\n"),
            Err(Issue::fail("No markdownlint-cli2 hook"))
        );
    }

    #[test]
    fn commit_hooks_run_in_the_pre_commit_stage() {
        let hook = |stages: &str, default_stages: &str| {
            commit_hook(&format!(
                "{default_stages}repos:\n  - repo: local\n    hooks: [{{id: markdownlint-cli2{stages}}}]\n"
            ))
        };
        for (stages, default_stages) in [
            ("", ""),
            ("", "default_stages: [pre-commit]\n"),
            ("", "default_stages: pre-commit\n"),
            (
                ", stages: [pre-commit, pre-push]",
                "default_stages: [pre-push]\n",
            ),
            // An empty list is no list, and commit is the stage's old name.
            (", stages: []", "default_stages: [pre-commit]\n"),
            (", stages: [commit]", ""),
            ("", "default_stages: [commit]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Ok(()),
                "{stages} {default_stages}"
            );
        }
        for (stages, default_stages) in [
            (", stages: [pre-push]", ""),
            (", stages: []", "default_stages: [pre-push]\n"),
            ("", "default_stages: [pre-push, manual]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Err(Issue::warn(
                    "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
                )),
                "{stages} {default_stages}"
            );
        }
        for (stages, default_stages) in [
            (", stages: [manual]", ""),
            (", stages: [manual]", "default_stages: [pre-commit]\n"),
            ("", "default_stages: [manual]\n"),
            (", stages: []", "default_stages: [manual]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Err(Issue::fail(
                    "The markdownlint-cli2 hook runs only in the manual stage, so it never runs on its own"
                )),
                "{stages} {default_stages}"
            );
        }
    }

    #[test]
    fn null_stages_fail_parsing() {
        let stages = |stages: &str| {
            serde_saphyr::from_str::<PrecommitConfig>(&format!(
                "repos:\n  - repo: local\n    hooks: [{{id: markdownlint-cli2{stages}}}]\n"
            ))
            .map(|config| {
                config
                    .repos
                    .into_iter()
                    .flat_map(|repo| repo.hooks)
                    .map(|hook| hook.stages)
                    .collect::<Vec<_>>()
            })
        };
        assert!(stages(", stages: ~").is_err());
        assert!(stages(", stages: null").is_err());
        // A key without a value, the likeliest way to write null by accident.
        assert!(
            serde_saphyr::from_str::<PrecommitConfig>(
                "repos:\n  - repo: local\n    hooks:\n      - id: markdownlint-cli2\n        stages:\n"
            )
            .is_err()
        );
        assert_eq!(stages("").unwrap(), [None]);
        assert_eq!(
            stages(", stages: [pre-commit]").unwrap(),
            [Some(vec!["pre-commit".to_owned()])]
        );
    }

    #[test]
    fn a_hook_in_manual_and_another_stage_still_runs() {
        assert_eq!(
            commit_hook(
                "repos:\n  - repo: local\n    hooks: [{id: markdownlint-cli2, stages: [manual, pre-push]}]\n"
            ),
            Err(Issue::warn(
                "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
            ))
        );
        assert_eq!(
            commit_hook(
                "repos:\n  - repo: local\n    hooks:\n      - {id: markdownlint-cli2, stages: [manual]}\n      \
                 - {id: markdownlint-cli2, stages: [pre-commit]}\n"
            ),
            Ok(())
        );
    }

    #[test]
    fn a_manual_hook_beside_one_in_another_stage_still_runs() {
        assert_eq!(
            commit_hook(
                "repos:\n  - repo: local\n    hooks:\n      - {id: markdownlint-cli2, stages: [manual]}\n      \
                 - {id: markdownlint-cli2, stages: [pre-push]}\n"
            ),
            Err(Issue::warn(
                "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
            ))
        );
    }

    #[test]
    fn default_stages_must_be_pre_commit_alone() {
        for yaml in [
            "default_stages: [pre-commit]\nrepos: []\n",
            "default_stages: ['pre-commit']\n",
        ] {
            assert_eq!(default_stages(yaml), Ok(()), "{yaml}");
        }
        for (yaml, issue) in [
            (
                "repos: []\n",
                Issue::warn(
                    "No default_stages, so hooks without stages of their own run in every installed stage",
                ),
            ),
            (
                "default_stages: [commit]\n",
                Issue::warn("default_stages is [commit], not [pre-commit]"),
            ),
            // pre-commit still knows each stage by its old name.
            (
                "default_stages: [merge-commit, push]\n",
                Issue::warn("default_stages is [merge-commit, push], not [pre-commit]"),
            ),
            (
                "default_stages: [pre-commit, pre-push]\n",
                Issue::warn("default_stages is [pre-commit, pre-push], not [pre-commit]"),
            ),
            (
                "default_stages: [manual]\n",
                Issue::warn("default_stages is [manual], not [pre-commit]"),
            ),
            (
                "default_stages: []\n",
                Issue::warn("default_stages is [], not [pre-commit]"),
            ),
            // pre-commit rejects each of these, and so runs no hooks at all.
            (
                "default_stages: pre-commit\n",
                Issue::fail("default_stages isn't a list"),
            ),
            (
                "default_stages:\n",
                Issue::fail("default_stages isn't a list"),
            ),
            (
                "default_stages: ~\n",
                Issue::fail("default_stages isn't a list"),
            ),
            (
                "default_stages: [pre-commit, pre_push, 3, ~]\n",
                Issue::fail(
                    "default_stages lists pre_push, 3, null, which pre-commit doesn't know",
                ),
            ),
            (
                "default_stages: [[pre-commit]]\n",
                Issue::fail("default_stages lists [\"pre-commit\"], which pre-commit doesn't know"),
            ),
        ] {
            assert_eq!(default_stages(yaml), Err(issue), "{yaml}");
        }
    }
}

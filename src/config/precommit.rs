// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `'.pre-commit-config.yaml`.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

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
    /// only the check reading it.
    pub default_install_hook_types: Option<Value>,
    /// The stages a hook runs in when it doesn't list its own, kept as YAML wrote it, so that a malformed value fails
    /// only the check reading it.
    pub default_stages: Option<Value>,
    pub repos: Vec<PrecommitRepo>,
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
    pub stages: Option<Vec<String>>,
    pub args: Vec<String>,
}

impl PrecommitHook {
    /// The words of the command the hook runs: its entry, or `default_entry` if it sets none, as a remote hook may
    /// leave it to the hook's repository, followed by the args pre-commit appends to it.
    pub(crate) fn command_words<'a>(&'a self, default_entry: &'a str) -> Vec<&'a str> {
        self.entry
            .as_deref()
            .unwrap_or(default_entry)
            .split_whitespace()
            .chain(self.args.iter().map(String::as_str))
            .collect()
    }
}

impl PrecommitConfig {
    /// Whether `default_stages` is exactly `[pre-commit]`, so that hooks without stages of their own run once per
    /// commit, and not again on its message.
    pub(crate) fn check_default_stages(&self) -> Result<(), String> {
        match &self.default_stages {
            None | Some(Value::Null) => Err(
                "No default_stages, so hooks without stages of their own run in every installed stage".to_owned(),
            ),
            Some(Value::Array(stages)) if matches!(stages.as_slice(), [stage] if stage == "pre-commit") => {
                Ok(())
            }
            Some(Value::Array(stages)) => Err(format!(
                "default_stages is [{}], not [pre-commit]",
                stages
                    .iter()
                    .map(|stage| stage.as_str().map_or_else(|| stage.to_string(), str::to_owned))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Some(_) => Err("default_stages isn't a list".to_owned()),
        }
    }

    /// Whether `default_install_hook_types` lists both `pre-commit` and `commit-msg`, so that a plain `pre-commit
    /// install` also installs the commit message hook. Any other hook types pre-commit knows may be listed too.
    pub(crate) fn check_default_install_hook_types(&self) -> Result<(), String> {
        const REQUIRED: [&str; 2] = ["pre-commit", "commit-msg"];
        match &self.default_install_hook_types {
            None | Some(Value::Null) => Err(
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
                    .map(|hook_type| hook_type.as_str().map_or_else(|| hook_type.to_string(), str::to_owned))
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
            if let Some(alias) = hook
                .alias
                .as_deref()
                .filter(|alias| !alias.is_empty() && *alias != hook.id)
            {
                names.push(alias);
            }
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

    /// Check if any hook with the given ID exists across all repos.
    pub(crate) fn has_hook(&self, hook_id: &str) -> bool {
        self.hooks(hook_id).next().is_some()
    }

    /// Every hook with the given ID, across all repos.
    pub(crate) fn hooks<'a>(&'a self, hook_id: &'a str) -> impl Iterator<Item = &'a PrecommitHook> {
        self.repos
            .iter()
            .flat_map(|r| &r.hooks)
            .filter(move |h| h.id == hook_id)
    }

    /// Check if any repo URL contains the given substring.
    pub(crate) fn has_repo_containing(&self, substring: &str) -> bool {
        self.repos.iter().any(|r| r.repo.contains(substring))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_stages(yaml: &str) -> Result<(), String> {
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
                "No default_install_hook_types, so pre-commit install installs only the pre-commit hook",
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

    #[test]
    fn default_stages_must_be_pre_commit_alone() {
        assert_eq!(
            default_stages("default_stages: [pre-commit]\nrepos: []\n"),
            Ok(())
        );
        assert_eq!(
            default_stages("repos: []\n"),
            Err("No default_stages, so hooks without stages of their own run in every installed stage".to_owned())
        );
        assert_eq!(
            default_stages("default_stages: [commit]\n"),
            Err("default_stages is [commit], not [pre-commit]".to_owned())
        );
        assert_eq!(
            default_stages("default_stages: [pre-commit, pre-push]\n"),
            Err("default_stages is [pre-commit, pre-push], not [pre-commit]".to_owned())
        );
        assert_eq!(
            default_stages("default_stages: pre-commit\n"),
            Err("default_stages isn't a list".to_owned())
        );
    }
}

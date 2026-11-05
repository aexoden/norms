// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `'.pre-commit-config.yaml`.

use serde::Deserialize;
use serde_json::Value;

/// Top-level `.pre-commit-config.yaml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrecommitConfig {
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
    pub name: Option<String>,
    pub entry: Option<String>,
    pub language: Option<String>,
    pub stages: Option<Vec<String>>,
    pub args: Vec<String>,
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

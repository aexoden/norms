// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `'.pre-commit-config.yaml`.

use serde::Deserialize;

/// Top-level `.pre-commit-config.yaml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrecommitConfig {
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

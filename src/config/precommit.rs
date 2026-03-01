// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `'.pre-commit-config.yaml`.

use serde::Deserialize;

/// Top-level `.pre-commit-config.yaml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct PrecommitConfig {
    pub repos: Vec<PrecommitRepo>,
}

/// A single repo entry in the pre-commit config.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct PrecommitRepo {
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
pub struct PrecommitHook {
    pub id: String,
    pub name: Option<String>,
    pub entry: Option<String>,
    pub language: Option<String>,
    pub stages: Option<Vec<String>>,
}

impl PrecommitConfig {
    /// Check if any hook with the given ID exists across all repos.
    pub fn has_hook(&self, hook_id: &str) -> bool {
        self.repos
            .iter()
            .flat_map(|r| &r.hooks)
            .any(|h| h.id == hook_id)
    }

    /// Check if any repo URL contains the given substring.
    pub fn has_repo_containing(&self, substring: &str) -> bool {
        self.repos.iter().any(|r| r.repo.contains(substring))
    }
}

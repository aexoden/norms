// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `devbox.json`.

use serde::Deserialize;

/// Top-level `devbox.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct DevboxConfig {
    /// JSON Schema reference.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,

    /// List of Nix packages
    #[serde(default)]
    pub packages: Vec<String>,

    /// Shell configuration.
    pub shell: Option<DevboxShell>,
}

/// The `shell` section of `devbox.json`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct DevboxShell {
    pub init_hook: Option<Vec<String>>,
    pub scripts: Option<serde_json::Value>,
}

impl DevboxConfig {
    /// Check if a particular package prefix is listed
    pub fn has_package(&self, prefix: &str) -> bool {
        self.packages.iter().any(|p| {
            p == prefix
                || p.starts_with(&format!("{prefix}@")) | p.starts_with(&format!("\"{prefix}"))
        })
    }
}

// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `devbox.json`.

use std::collections::HashMap;

use serde::Deserialize;
use serde::de::Deserializer;

/// Top-level `devbox.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct DevboxConfig {
    /// JSON Schema reference.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,

    /// List of Nix packages (normalized from either a list or a dictionary).
    #[serde(default, deserialize_with = "deserialize_packages")]
    pub packages: Vec<String>,

    /// Map of environment variables.
    pub env: Option<HashMap<String, String>>,

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

/// Deserialize `packages` from either a list of strings or a dictionary.
///
/// In dictionary form, the keys are package names and the values are either a
/// version string or an object with properties like `version` and `outputs`.
/// Only the keys are extracted.
fn deserialize_packages<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Packages {
        List(Vec<String>),
        Map(serde_json::Map<String, serde_json::Value>),
    }

    match Packages::deserialize(deserializer)? {
        Packages::List(list) => Ok(list),
        Packages::Map(map) => Ok(map.keys().cloned().collect()),
    }
}

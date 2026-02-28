// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `renovate.json`.

use serde::Deserialize;

/// Top-level `renovate.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RenovateConfig {
    /// JSON Schema reference.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,

    /// Preset configurations to extend.
    #[serde(default)]
    pub extends: Vec<String>,
}

impl RenovateConfig {
    /// Whether `extends` includes any preset containing `"best-practices"`.
    pub fn extends_best_practices(&self) -> bool {
        self.extends.iter().any(|e| e.contains("best-practices"))
    }
}

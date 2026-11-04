// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `renovate.json`.

use serde::Deserialize;

/// The shared preset every `renovate.json` extends.
pub(crate) const SHARED_PRESET: &str = "github>aexoden/renovate-config";

/// Top-level `renovate.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct RenovateConfig {
    /// JSON Schema reference.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,

    /// Preset configurations to extend.
    #[serde(default)]
    pub extends: Vec<String>,
}

impl RenovateConfig {
    /// Whether `extends` includes the shared preset's default preset.
    pub(crate) fn extends_shared_preset(&self) -> bool {
        self.extends.iter().any(|e| is_shared_preset(e))
    }
}

/// Whether `preset` names the shared preset's default preset, optionally as
/// `:default` and pinned to a ref (`#v1.2.0`). Named presets from the same
/// repository (`:rust`, `//path/preset`) are not the default one. Only these
/// canonical spellings are recognized, not equivalents such as `//default`,
/// `:default.json` or `local>`.
fn is_shared_preset(preset: &str) -> bool {
    let preset = match preset.split_once('#') {
        Some((preset, tag)) if !tag.is_empty() => preset,
        Some(_) => return false,
        None => preset,
    };
    let preset = preset.strip_suffix(":default").unwrap_or(preset);

    // Renovate matches the platform exactly, but GitHub owner and repository
    // names are case-insensitive.
    match (preset.split_once('>'), SHARED_PRESET.split_once('>')) {
        (Some((platform, repo)), Some((shared_platform, shared_repo))) => {
            platform == shared_platform && repo.eq_ignore_ascii_case(shared_repo)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_default_preset() {
        for preset in [
            "github>aexoden/renovate-config",
            "github>aexoden/renovate-config:default",
            "github>aexoden/renovate-config#v1.2.0",
            "github>aexoden/renovate-config:default#v1.2.0",
            "github>Aexoden/Renovate-Config",
        ] {
            assert!(is_shared_preset(preset), "{preset}");
        }
    }

    #[test]
    fn rejects_other_presets() {
        for preset in [
            "config:best-practices",
            "github>aexoden/renovate-config:rust",
            "github>aexoden/renovate-config//rust/preset",
            "github>aexoden/renovate-config#",
            "github>aexoden/renovate-config-fork",
            "github>someone/renovate-config",
            "gitlab>aexoden/renovate-config",
            "GitHub>aexoden/renovate-config",
            "github>aexoden/renovate-config:DEFAULT",
        ] {
            assert!(!is_shared_preset(preset), "{preset}");
        }
    }

    #[test]
    fn finds_the_preset_among_others() {
        let config = RenovateConfig {
            extends: vec!["config:best-practices".to_owned(), SHARED_PRESET.to_owned()],
            ..RenovateConfig::default()
        };
        assert!(config.extends_shared_preset());
        assert!(!RenovateConfig::default().extends_shared_preset());
    }
}

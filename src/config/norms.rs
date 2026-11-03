// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `norms.toml`, the project's norms configuration.
//!
//! Unlike the models of other tools' files, this one rejects unknown keys: norms owns the format, so a misspelled key
//! should fail validation rather than be silently ignored.

use std::collections::{BTreeSet, HashSet};

use serde::Deserialize;

/// Top-level `norms.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct NormsConfig {
    /// Checks the project departs from, each with its rationale.
    pub waivers: Vec<Waiver>,
}

impl NormsConfig {
    /// The checks named by waivers that give no reason.
    pub(crate) fn unexplained_waivers(&self) -> Vec<&str> {
        self.waivers
            .iter()
            .filter(|waiver| waiver.reason.trim().is_empty())
            .map(|waiver| waiver.check.as_str())
            .collect()
    }

    /// The checks named by more than one waiver.
    pub(crate) fn duplicate_waivers(&self) -> BTreeSet<&str> {
        let mut seen = HashSet::new();

        self.waivers
            .iter()
            .map(|waiver| waiver.check.as_str())
            .filter(|&check| !seen.insert(check))
            .collect()
    }
}

/// A `[[waivers]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Waiver {
    /// The ID of the waived check.
    pub check: String,
    /// Why the project departs from the standard.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_valid() {
        let config: NormsConfig = toml::from_str("").unwrap();
        assert!(config.waivers.is_empty());
    }

    #[test]
    fn parses_waivers() {
        let config: NormsConfig = toml::from_str(
            r#"
            [[waivers]]
            check = "cargo-toml-rust-version"
            reason = "Applications don't need an MSRV."
            "#,
        )
        .unwrap();

        let checks: Vec<_> = config.waivers.iter().map(|w| w.check.as_str()).collect();
        assert_eq!(checks, ["cargo-toml-rust-version"]);
    }

    #[test]
    fn rejects_unknown_keys() {
        for toml in [
            "[components]\ntypescript = \"frontend\"",
            "[[waivers]]\ncheck = \"cargo-toml-rust-version\"\nreason = \"x\"\nexpires = \"never\"",
            "[waiver]\ncheck = \"cargo-toml-rust-version\"",
        ] {
            assert!(toml::from_str::<NormsConfig>(toml).is_err(), "{toml}");
        }
    }

    #[test]
    fn finds_unexplained_and_duplicate_waivers() {
        let config: NormsConfig = toml::from_str(
            r#"
            waivers = [
                { check = "a", reason = "Fine." },
                { check = "b", reason = " " },
                { check = "a", reason = "Fine." },
                { check = "c", reason = "" },
                { check = "b", reason = "Fine." },
                { check = "a", reason = "Fine." },
            ]
            "#,
        )
        .unwrap();

        assert_eq!(config.unexplained_waivers(), ["b", "c"]);
        assert_eq!(
            config.duplicate_waivers().into_iter().collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn requires_a_reason() {
        assert!(
            toml::from_str::<NormsConfig>("[[waivers]]\ncheck = \"cargo-toml-rust-version\"")
                .is_err()
        );
    }
}

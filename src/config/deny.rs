// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for cargo-deny's `deny.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. The values checks read are kept unparsed, so that a
//! malformed one fails only the check reading it; a table of the wrong type fails parsing.

use serde::Deserialize;

/// Top-level `deny.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct DenyToml {
    pub advisories: Option<DenyAdvisories>,
    pub bans: Option<DenyBans>,
}

/// The `[advisories]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct DenyAdvisories {
    /// The advisories to ignore: each an advisory ID or crate, or a table naming one with a `reason`.
    pub ignore: Option<toml::Value>,
}

/// The `[bans]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct DenyBans {
    pub multiple_versions: Option<toml::Value>,
}

impl DenyToml {
    /// Check that `[bans] multiple-versions` is `"allow"`. cargo-deny's default, `"warn"`, fails the check under
    /// `-D warnings`.
    pub(crate) fn check_multiple_versions(&self) -> Result<(), String> {
        match self
            .bans
            .as_ref()
            .and_then(|bans| bans.multiple_versions.as_ref())
        {
            Some(value) if value.as_str() == Some("allow") => Ok(()),
            Some(value) => Err(format!("multiple-versions is {value}, not \"allow\"")),
            None => Err("multiple-versions isn't set, so cargo-deny warns".to_owned()),
        }
    }

    /// Check that every entry in `[advisories] ignore` is a table with a non-empty `reason`. The error names each entry
    /// that isn't, by its `id` or `crate` where it has one.
    pub(crate) fn check_ignore_reasons(&self) -> Result<(), String> {
        let Some(ignore) = self
            .advisories
            .as_ref()
            .and_then(|advisories| advisories.ignore.as_ref())
        else {
            return Ok(());
        };
        let toml::Value::Array(entries) = ignore else {
            return Err(format!("ignore is {ignore}, not an array"));
        };

        let unexplained: Vec<String> = entries
            .iter()
            .filter(|entry| {
                entry
                    .get("reason")
                    .and_then(toml::Value::as_str)
                    .is_none_or(|reason| reason.trim().is_empty())
            })
            .map(|entry| {
                entry
                    .as_str()
                    .or_else(|| {
                        ["id", "crate"]
                            .into_iter()
                            .find_map(|key| entry.get(key).and_then(toml::Value::as_str))
                    })
                    .map_or_else(|| entry.to_string(), str::to_owned)
            })
            .collect();

        if unexplained.is_empty() {
            Ok(())
        } else {
            Err(format!("No reason given: {}", unexplained.join(", ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny(toml: &str) -> DenyToml {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn multiple_versions_are_allowed() {
        assert_eq!(
            deny("[bans]\nmultiple-versions = \"allow\"\n").check_multiple_versions(),
            Ok(())
        );
        assert_eq!(
            deny("[bans]\nmultiple-versions = \"deny\"\n").check_multiple_versions(),
            Err("multiple-versions is \"deny\", not \"allow\"".to_owned())
        );
        assert_eq!(
            deny("[licenses]\nallow = [\"MIT\"]\n").check_multiple_versions(),
            Err("multiple-versions isn't set, so cargo-deny warns".to_owned())
        );
    }

    #[test]
    fn ignored_advisories_give_reasons() {
        assert_eq!(deny("").check_ignore_reasons(), Ok(()));
        assert_eq!(
            deny("[advisories]\nignore = []\n").check_ignore_reasons(),
            Ok(())
        );
        assert_eq!(
            deny(
                r#"
                [advisories]
                ignore = [
                    { id = "RUSTSEC-2024-0001", reason = "Not reachable." },
                    { crate = "atty@0.2.14", reason = "Only in a dev tool." },
                ]
                "#
            )
            .check_ignore_reasons(),
            Ok(())
        );
        assert_eq!(
            deny(
                r#"
                [advisories]
                ignore = [
                    "RUSTSEC-2024-0002",
                    { id = "RUSTSEC-2024-0003" },
                    { crate = "atty", reason = " " },
                    { id = "RUSTSEC-2024-0004", reason = "Explained." },
                    { reason = 1 },
                ]
                "#
            )
            .check_ignore_reasons(),
            Err(
                "No reason given: RUSTSEC-2024-0002, RUSTSEC-2024-0003, atty, { reason = 1 }"
                    .to_owned()
            )
        );
        assert_eq!(
            deny("[advisories]\nignore = \"RUSTSEC-2024-0002\"\n").check_ignore_reasons(),
            Err("ignore is \"RUSTSEC-2024-0002\", not an array".to_owned())
        );
    }
}

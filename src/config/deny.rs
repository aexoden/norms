// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for cargo-deny's `deny.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. The values checks read are kept unparsed, so that a
//! malformed one fails only the check reading it; a table of the wrong type fails parsing.

use std::path::Path;

use serde::Deserialize;

/// The file that holds cargo-deny's configuration.
pub(crate) const DENY_TOML: &str = "deny.toml";

/// The other names cargo-deny reads its configuration from, after [`DENY_TOML`] within one directory, in the order it
/// tries them. The standard doesn't use them.
pub(crate) const OTHER_DENY_TOMLS: [&str; 2] = [".deny.toml", ".cargo/deny.toml"];

/// Whether `path` names one of the [`OTHER_DENY_TOMLS`] rather than [`DENY_TOML`].
pub(crate) fn is_other_deny_toml(path: &Path) -> bool {
    OTHER_DENY_TOMLS.iter().any(|name| path.ends_with(name))
}

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
    pub std_replacements: Option<toml::Value>,
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

    /// Check that `[bans] std-replacements` is a table that leaves `level` and `scope` at their defaults, so that
    /// cargo-deny fails on a dependency of the workspace's own crates that `std` or `core` replaces. Without the table,
    /// cargo-deny skips the check. Other keys, such as `ignore`, may be set.
    pub(crate) fn check_std_replacements(&self) -> Result<(), String> {
        match self
            .bans
            .as_ref()
            .and_then(|bans| bans.std_replacements.as_ref())
        {
            Some(toml::Value::Table(table)) => {
                let changed: Vec<String> = ["level", "scope"]
                    .into_iter()
                    .filter_map(|key| table.get(key).map(|value| format!("{key} = {value}")))
                    .collect();
                if changed.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "std-replacements sets {}, which the standard leaves at cargo-deny's defaults",
                        changed.join(", ")
                    ))
                }
            }
            Some(value) => Err(format!("std-replacements is {value}, not a table")),
            None => Err("std-replacements isn't set, so cargo-deny skips the check".to_owned()),
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
    fn std_replacements_are_checked() {
        assert_eq!(
            deny("[bans]\nstd-replacements = {}\n").check_std_replacements(),
            Ok(())
        );
        assert_eq!(
            deny("[bans.std-replacements]\nignore = [\"once_cell\"]\n").check_std_replacements(),
            Ok(())
        );
        assert_eq!(
            deny("[bans]\nstd-replacements = { level = \"allow\", scope = \"none\" }\n")
                .check_std_replacements(),
            Err(
                "std-replacements sets level = \"allow\", scope = \"none\", which the standard leaves at \
                 cargo-deny's defaults"
                    .to_owned()
            )
        );
        assert_eq!(
            deny("[bans.std-replacements]\nlevel = \"warn\"\n").check_std_replacements(),
            Err(
                "std-replacements sets level = \"warn\", which the standard leaves at cargo-deny's defaults"
                    .to_owned()
            )
        );
        assert_eq!(
            deny("[bans]\nstd-replacements = true\n").check_std_replacements(),
            Err("std-replacements is true, not a table".to_owned())
        );
        assert_eq!(
            deny("[bans]\nmultiple-versions = \"allow\"\n").check_std_replacements(),
            Err("std-replacements isn't set, so cargo-deny skips the check".to_owned())
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

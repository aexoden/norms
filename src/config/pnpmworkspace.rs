// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `pnpm-workspace.yaml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use serde::Deserialize;
use serde_json::Value;

use super::release_age::check_release_age_minutes;

/// Top-level `pnpm-workspace.yaml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PnpmWorkspace {
    /// How old, in minutes, a release must be before pnpm installs it. Kept unparsed, so that a malformed value fails
    /// only the check reading it.
    pub minimum_release_age: Option<Value>,
}

impl PnpmWorkspace {
    /// Check that `minimumReleaseAge` is the release age. pnpm reads it as a JavaScript number, so `4320.0` and
    /// `"4320"` mean the same.
    pub(crate) fn check_minimum_release_age(&self) -> Result<(), String> {
        let value = self
            .minimum_release_age
            .as_ref()
            .ok_or("Missing minimumReleaseAge")?;
        let minutes = match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
        }
        .ok_or_else(|| format!("minimumReleaseAge {value} isn't a number"))?;

        check_release_age_minutes(minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(yaml: &str) -> Result<(), String> {
        serde_saphyr::from_str::<PnpmWorkspace>(yaml)
            .unwrap()
            .check_minimum_release_age()
    }

    #[test]
    fn accepts_the_release_age() {
        for yaml in [
            "minimumReleaseAge: 4320\n",
            "minimumReleaseAge: 4320.0\n",
            "minimumReleaseAge: \"4320\"\n",
            "allowBuilds: {}\nminimumReleaseAge: 4320\n",
        ] {
            assert_eq!(check(yaml), Ok(()), "{yaml}");
        }
    }

    #[test]
    fn rejects_a_missing_or_other_age() {
        assert_eq!(
            check("allowBuilds: {}\n"),
            Err("Missing minimumReleaseAge".to_owned())
        );
        assert_eq!(
            check("minimumReleaseAge: 1440\n"),
            Err("1440 minutes isn't 3 days (4320)".to_owned())
        );
        assert_eq!(
            check("minimumReleaseAge: [4320]\n"),
            Err("minimumReleaseAge [4320] isn't a number".to_owned())
        );
        assert!(check("minimumReleaseAge: three days\n").is_err());
    }
}

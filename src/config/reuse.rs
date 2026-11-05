// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `REUSE.toml`.

use serde::Deserialize;

/// The `REUSE.toml` format version that the REUSE specification defines.
pub(crate) const REUSE_TOML_VERSION: i64 = 1;

/// Top-level `REUSE.toml` structure. Only the version is read; the annotations are left to `reuse lint`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ReuseToml {
    /// The format version, read leniently so that one of the wrong type fails only its check.
    pub version: Option<toml::Value>,
}

impl ReuseToml {
    /// Check that `version` is the one the REUSE specification defines.
    pub(crate) fn check_version(&self) -> Result<(), String> {
        match &self.version {
            Some(toml::Value::Integer(REUSE_TOML_VERSION)) => Ok(()),
            Some(version) => Err(format!("version is {version}, not {REUSE_TOML_VERSION}")),
            None => Err("No version".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(toml: &str) -> Result<(), String> {
        toml::from_str::<ReuseToml>(toml).unwrap().check_version()
    }

    #[test]
    fn requires_version_one() {
        assert_eq!(check("version = 1\n"), Ok(()));
        assert_eq!(
            check("version = 2\n"),
            Err("version is 2, not 1".to_owned())
        );
        assert_eq!(
            check("version = \"1\"\n"),
            Err("version is \"1\", not 1".to_owned())
        );
        assert_eq!(
            check("[[annotations]]\npath = \"x\"\n"),
            Err("No version".to_owned())
        );
    }
}

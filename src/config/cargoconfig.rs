// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `.cargo/config.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error. Values are kept unparsed, so that a malformed one
//! fails only the check reading it.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::release_age::check_release_age_value;

/// Top-level `.cargo/config.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoConfig {
    pub registry: Option<CargoRegistryConfig>,
    /// Settings for each registry, by name.
    pub registries: BTreeMap<String, CargoRegistriesEntry>,
    pub resolver: Option<CargoResolverConfig>,
}

/// The `[registry]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoRegistryConfig {
    /// The release age for every registry that doesn't set its own.
    pub global_min_publish_age: Option<toml::Value>,
    /// The release age for crates.io, overriding the global one.
    pub min_publish_age: Option<toml::Value>,
}

/// One registry's entry in `[registries]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoRegistriesEntry {
    pub min_publish_age: Option<toml::Value>,
}

/// The `[resolver]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoResolverConfig {
    /// What the resolver does with a release younger than the release age: `"deny"` it (the default) or `"allow"`
    /// it.
    pub incompatible_publish_age: Option<toml::Value>,
}

impl CargoConfig {
    /// Check that `[registry] global-min-publish-age` is the release age, and that nothing overrides it: neither a
    /// registry's own `min-publish-age` with another age, nor `[resolver] incompatible-publish-age = "allow"`.
    pub(crate) fn check_min_publish_age(&self) -> Result<(), String> {
        let registry = self.registry.as_ref();
        let global = registry
            .and_then(|r| r.global_min_publish_age.as_ref())
            .ok_or("Missing [registry] global-min-publish-age")?;
        check_release_age_value(global).map_err(|e| format!("global-min-publish-age: {e}"))?;

        if let Some(age) = registry.and_then(|r| r.min_publish_age.as_ref()) {
            check_release_age_value(age).map_err(|e| format!("registry.min-publish-age: {e}"))?;
        }

        self.registries.iter().try_for_each(|(name, entry)| {
            entry
                .min_publish_age
                .as_ref()
                .map_or(Ok(()), check_release_age_value)
                .map_err(|e| format!("registries.{name}.min-publish-age: {e}"))
        })?;

        let resolver = self
            .resolver
            .as_ref()
            .and_then(|r| r.incompatible_publish_age.as_ref());
        if resolver.and_then(toml::Value::as_str) == Some("allow") {
            return Err(
                "[resolver] incompatible-publish-age = \"allow\" turns the release age off"
                    .to_owned(),
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(config: &str) -> Result<(), String> {
        toml::from_str::<CargoConfig>(config)
            .unwrap()
            .check_min_publish_age()
    }

    const GLOBAL: &str = "[registry]\nglobal-min-publish-age = \"3 days\"\n";

    #[test]
    fn accepts_the_release_age() {
        assert_eq!(check(GLOBAL), Ok(()));
        assert_eq!(
            check(&format!(
                "{GLOBAL}min-publish-age = \"72 hours\"\n\n[registries.private]\nmin-publish-age = \"3 days\"\n\n\
                 [resolver]\nincompatible-publish-age = \"deny\"\n"
            )),
            Ok(())
        );
    }

    #[test]
    fn rejects_a_missing_or_other_age() {
        assert_eq!(
            check(""),
            Err("Missing [registry] global-min-publish-age".to_owned())
        );
        assert_eq!(
            check("[registry]\nglobal-min-publish-age = \"7 days\"\n"),
            Err("global-min-publish-age: \"7 days\" isn't 3 days".to_owned())
        );
        assert_eq!(
            check("[registry]\nglobal-min-publish-age = 0\n"),
            Err("global-min-publish-age: 0 isn't a string".to_owned())
        );
    }

    #[test]
    fn rejects_an_override() {
        assert_eq!(
            check(&format!("{GLOBAL}min-publish-age = \"0\"\n")),
            Err("registry.min-publish-age: \"0\" isn't 3 days".to_owned())
        );
        assert_eq!(
            check(&format!(
                "{GLOBAL}\n[registries.crates-io]\nmin-publish-age = \"1 day\"\n"
            )),
            Err("registries.crates-io.min-publish-age: \"1 day\" isn't 3 days".to_owned())
        );
        assert_eq!(
            check(&format!(
                "{GLOBAL}\n[resolver]\nincompatible-publish-age = \"allow\"\n"
            )),
            Err(
                "[resolver] incompatible-publish-age = \"allow\" turns the release age off"
                    .to_owned()
            )
        );
    }
}

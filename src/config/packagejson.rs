// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `package.json`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

/// Top-level `package.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PackageJson {
    pub name: Option<String>,
    pub description: Option<String>,

    /// Module type (`"module"` for ESM, `"commonjs"` or absent for CJS).
    #[serde(rename = "type")]
    pub module_type: Option<String>,

    /// The tools needed to develop the package, such as Node.js and pnpm. Kept
    /// unparsed, so that a malformed entry fails only the check reading it
    /// (see [`PackageJson::dev_engine`]).
    pub dev_engines: Value,

    /// npm scripts.
    pub scripts: Option<HashMap<String, String>>,

    /// Production dependencies.
    pub dependencies: Option<HashMap<String, String>>,

    // Development dependencies.
    pub dev_dependencies: Option<HashMap<String, String>>,
}

/// One dependency in `devEngines`, such as `{ "name": "node", "version":
/// "25.9.0", "onFail": "download" }`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct DevEngine {
    pub name: Option<String>,
    pub version: Option<String>,
    pub on_fail: Option<String>,
}

impl DevEngine {
    /// Check that this dependency pins an exact version, which pnpm downloads
    /// if it isn't the one running. Build metadata isn't part of a version
    /// pnpm can download, so it doesn't count as exact.
    pub(crate) fn check_pinned(&self) -> Result<(), String> {
        let version = self.version.as_deref().ok_or("No version given")?;
        if !semver::Version::parse(version).is_ok_and(|v| v.build.is_empty()) {
            return Err(format!(
                "Version '{version}' isn't an exact version (x.y.z)"
            ));
        }

        match self.on_fail.as_deref() {
            Some("download") => Ok(()),
            Some(on_fail) => Err(format!("onFail is '{on_fail}', expected 'download'")),
            None => Err("No onFail given, expected 'download'".to_owned()),
        }
    }
}

/// Check that the `devEngines` entry `field` names `name` and pins it. Every
/// alternative naming it must be pinned.
pub(crate) fn check_dev_engine(
    config: &PackageJson,
    field: &str,
    name: &str,
) -> Result<(), String> {
    let engines = config.dev_engine(field)?;
    let mut named = engines
        .iter()
        .filter(|e| e.name.as_deref() == Some(name))
        .peekable();

    if named.peek().is_none() {
        return Err(format!("devEngines.{field} doesn't name {name}"));
    }

    named.try_for_each(DevEngine::check_pinned)
}

impl PackageJson {
    /// The dependencies of the `devEngines` entry `field`, which npm allows to
    /// be one object or an array of alternatives.
    pub(crate) fn dev_engine(&self, field: &str) -> Result<Vec<DevEngine>, String> {
        let entries = match self.dev_engines.get(field) {
            None | Some(Value::Null) => return Err(format!("Missing devEngines.{field}")),
            Some(Value::Array(entries)) if entries.is_empty() => {
                return Err(format!("Missing devEngines.{field}"));
            }
            Some(Value::Array(entries)) => entries.iter().collect(),
            Some(entry) => vec![entry],
        };

        entries
            .into_iter()
            .map(|entry| {
                // serde would read an array as a struct's fields in order.
                if !entry.is_object() {
                    return Err(format!("devEngines.{field} holds a non-object entry"));
                }
                DevEngine::deserialize(entry)
                    .map_err(|e| format!("devEngines.{field} has an invalid entry: {e}"))
            })
            .collect()
    }

    /// Whether `type` is set to `"module"` (ESM).
    pub(crate) fn is_esm(&self) -> bool {
        self.module_type.as_deref() == Some("module")
    }

    /// Whether a given package name appears in any dependency list.
    pub(crate) fn has_dep(&self, name: &str) -> bool {
        self.dependencies
            .as_ref()
            .is_some_and(|d| d.contains_key(name))
            || self
                .dev_dependencies
                .as_ref()
                .is_some_and(|d| d.contains_key(name))
    }

    /// Whether any dependency name starts with the given prefix.
    pub(crate) fn has_dep_starting_with(&self, prefix: &str) -> bool {
        let check = |deps: &HashMap<String, String>| deps.keys().any(|k| k.starts_with(prefix));
        self.dependencies.as_ref().is_some_and(check)
            || self.dev_dependencies.as_ref().is_some_and(check)
    }

    /// Whether a script with the given name exists.
    pub(crate) fn has_script(&self, name: &str) -> bool {
        self.scripts.as_ref().is_some_and(|s| s.contains_key(name))
    }

    /// Whether any script name matches one of the given alternatives.
    pub(crate) fn has_any_script(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.has_script(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> PackageJson {
        serde_json::from_str(json).unwrap()
    }

    fn check_runtime(runtime: &str) -> Result<(), String> {
        let json = format!(r#"{{ "devEngines": {{ "runtime": {runtime} }} }}"#);
        check_dev_engine(&parse(&json), "runtime", "node")
    }

    fn node(version: &str) -> String {
        format!(r#"{{ "name": "node", "version": "{version}", "onFail": "download" }}"#)
    }

    #[test]
    fn accepts_a_pinned_engine() {
        let config = parse(
            r#"{ "devEngines": {
                "runtime": { "name": "node", "version": "25.9.0", "onFail": "download" },
                "packageManager": { "name": "pnpm", "version": "12.7.0", "onFail": "download" }
            } }"#,
        );

        assert_eq!(check_dev_engine(&config, "runtime", "node"), Ok(()));
        assert_eq!(check_dev_engine(&config, "packageManager", "pnpm"), Ok(()));
        assert_eq!(check_runtime(&node("25.9.0-rc.1")), Ok(()));
    }

    #[test]
    fn finds_the_engine_among_alternatives() {
        let alternatives = format!(
            r#"[{{ "name": "deno", "version": ">=2" }}, {}]"#,
            node("25.9.0")
        );
        assert_eq!(check_runtime(&alternatives), Ok(()));

        // Every entry naming node must be pinned, not only the first.
        let duplicates = format!("[{}, {}]", node("25.9.0"), node(">=25"));
        assert!(check_runtime(&duplicates).is_err());
    }

    #[test]
    fn rejects_a_missing_or_other_engine() {
        for json in [
            "{}",
            r#"{ "devEngines": null }"#,
            r#"{ "devEngines": "node" }"#,
            r#"{ "devEngines": {} }"#,
            r#"{ "devEngines": { "runtime": null } }"#,
            r#"{ "devEngines": { "runtime": [] } }"#,
            r#"{ "engines": { "node": "25.9.0" } }"#,
        ] {
            assert_eq!(
                check_dev_engine(&parse(json), "runtime", "node"),
                Err("Missing devEngines.runtime".to_owned()),
                "{json}"
            );
        }

        let config = parse(
            r#"{ "devEngines": { "packageManager": { "name": "npm", "version": "11.0.0", "onFail": "download" } } }"#,
        );
        assert_eq!(
            check_dev_engine(&config, "packageManager", "pnpm"),
            Err("devEngines.packageManager doesn't name pnpm".to_owned())
        );
    }

    #[test]
    fn rejects_a_malformed_entry() {
        for runtime in [r#""node""#, r#"["node", "25.9.0", "download"]"#, "[1]"] {
            assert_eq!(
                check_runtime(runtime),
                Err("devEngines.runtime holds a non-object entry".to_owned()),
                "{runtime}"
            );
        }

        let error = check_runtime(r#"{ "name": "node", "version": 25 }"#).unwrap_err();
        assert!(
            error.starts_with("devEngines.runtime has an invalid entry: "),
            "{error}"
        );
    }

    #[test]
    fn rejects_an_unpinned_version() {
        for version in [
            ">=25",
            "^25.9.0",
            "25",
            "25.9",
            "v25.9.0",
            "25.9.0+build.5",
            "latest",
        ] {
            assert_eq!(
                check_runtime(&node(version)),
                Err(format!(
                    "Version '{version}' isn't an exact version (x.y.z)"
                )),
                "{version}"
            );
        }

        assert_eq!(
            check_runtime(r#"{ "name": "node", "onFail": "download" }"#),
            Err("No version given".to_owned())
        );
    }

    #[test]
    fn requires_download_on_fail() {
        for (runtime, expected) in [
            (
                r#"{ "name": "node", "version": "25.9.0", "onFail": "error" }"#,
                "onFail is 'error', expected 'download'",
            ),
            (
                r#"{ "name": "node", "version": "25.9.0" }"#,
                "No onFail given, expected 'download'",
            ),
        ] {
            assert_eq!(
                check_runtime(runtime),
                Err(expected.to_owned()),
                "{runtime}"
            );
        }
    }
}

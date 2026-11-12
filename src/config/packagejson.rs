// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `package.json`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::release::Release;

/// Top-level `package.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PackageJson {
    /// The package's version. Kept unparsed, so that a value of the wrong type is reported rather than failing to
    /// parse.
    pub version: Option<Value>,

    pub description: Option<String>,

    /// The license, as an SPDX expression. Kept unparsed, so that one of the wrong type fails only its check.
    pub license: Option<Value>,

    /// Module type (`"module"` for ESM, `"commonjs"` or absent for CJS).
    #[serde(rename = "type")]
    pub module_type: Option<String>,

    /// Whether npm refuses to publish the package. Kept unparsed, so that a value of the wrong type, such as
    /// `"true"`, is reported rather than failing to parse.
    pub private: Option<Value>,

    /// The tools needed to develop the package, such as Node.js and pnpm. Kept
    /// unparsed, so that a malformed entry fails only the check reading it
    /// (see [`PackageJson::dev_engine`]).
    pub dev_engines: Value,

    /// Corepack's package manager declaration (such as `"pnpm@12.7.0"`),
    /// which the standards replace with `devEngines.packageManager`. Kept
    /// unparsed, so that a value of any type is reported rather than failing
    /// to parse.
    pub package_manager: Option<Value>,

    /// npm scripts.
    pub scripts: Option<HashMap<String, String>>,

    /// Production dependencies.
    pub dependencies: Option<HashMap<String, String>>,

    // Development dependencies.
    pub dev_dependencies: Option<HashMap<String, String>>,

    /// Dependencies whose installation may fail without failing the install.
    pub optional_dependencies: Option<HashMap<String, String>>,

    /// Prettier's configuration, which Prettier reads in place of any configuration file beside it.
    pub prettier: Option<Value>,
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

/// Check that a dependency's specifier pins an exact version (`x.y.z`, without `^`, `~` or a range).
///
/// An alias to a registry package (`npm:name@x.y.z`, or `jsr:`) is checked by its version. Other protocols, such as
/// `workspace:`, `link:`, `file:`, `catalog:` and Git or tarball URLs, and local paths, tarballs and GitHub
/// `owner/repo` shorthands, don't name a registry version, so they pass.
fn check_specifier_exact(specifier: &str) -> Result<(), String> {
    // npm matches protocols regardless of case.
    let alias = ["npm:", "jsr:"].iter().find_map(|protocol| {
        let (prefix, rest) = specifier.split_at_checked(protocol.len())?;
        prefix.eq_ignore_ascii_case(protocol).then_some(rest)
    });
    let lowercase = specifier.to_ascii_lowercase();

    let version = if let Some(alias) = alias {
        // The version follows the last `@`, past a scope's leading one; without one, the alias names no version.
        let unscoped = alias.strip_prefix('@').unwrap_or(alias);
        unscoped
            .rsplit_once('@')
            .map_or(unscoped, |(_, version)| version)
    } else if specifier.contains([':', '/', '\\'])
        || specifier.starts_with('.')
        || [".tgz", ".tar", ".tar.gz"]
            .iter()
            .any(|extension| lowercase.ends_with(extension))
    {
        // A protocol, URL, path or tarball, as npm-package-arg reads them.
        return Ok(());
    } else {
        specifier
    };

    if semver::Version::parse(version).is_ok_and(|v| v.build.is_empty()) {
        Ok(())
    } else {
        Err(format!("\"{specifier}\""))
    }
}

impl PackageJson {
    /// Check that every dependency, development dependency and optional dependency is pinned exactly. Peer
    /// dependencies are ranges by design, so they aren't checked.
    ///
    /// The error lists each dependency that isn't, by its field and name, with its specifier.
    pub(crate) fn check_dependencies_exact(&self) -> Result<(), String> {
        let mut unpinned = Vec::new();
        for (field, dependencies) in [
            ("dependencies", &self.dependencies),
            ("devDependencies", &self.dev_dependencies),
            ("optionalDependencies", &self.optional_dependencies),
        ] {
            let mut names: Vec<_> = dependencies.iter().flatten().collect();
            names.sort();
            unpinned.extend(names.into_iter().filter_map(|(name, specifier)| {
                check_specifier_exact(specifier)
                    .err()
                    .map(|specifier| format!("{field}.{name} ({specifier})"))
            }));
        }

        if unpinned.is_empty() {
            Ok(())
        } else {
            Err(format!("Not pinned with x.y.z: {}", unpinned.join(", ")))
        }
    }

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

    /// Check that Corepack's `packageManager` field isn't set, so that
    /// `devEngines.packageManager` is the only pnpm pin. Corepack and some CI
    /// setup actions read the field, and a second pin can drift from the
    /// first.
    pub(crate) fn check_no_package_manager(&self) -> Result<(), String> {
        self.package_manager.as_ref().map_or(Ok(()), |value| {
            Err(format!(
                "Sets Corepack's packageManager ({value}); pin pnpm only in devEngines.packageManager"
            ))
        })
    }

    /// Check that `version` is that of the project's latest release, `release`.
    pub(crate) fn check_version(&self, release: &Release) -> Result<(), String> {
        match &self.version {
            Some(Value::String(version)) => release.check(Some(version)),
            Some(value) => Err(format!("version is {value}, not a string")),
            None => release.check(None),
        }
    }

    /// Check that `private` is `true`, so that npm refuses to publish the application by mistake. npm checks for
    /// exactly `true`, so a string such as `"true"` doesn't count.
    pub(crate) fn check_private(&self) -> Result<(), String> {
        match &self.private {
            Some(Value::Bool(true)) => Ok(()),
            Some(value) => Err(format!("private is {value}, not true")),
            None => Err("Missing \"private\": true".to_owned()),
        }
    }

    /// Check that `type` is `"module"`, so that Node.js loads the package's files as ES modules.
    pub(crate) fn check_module_type(&self) -> Result<(), String> {
        match self.module_type.as_deref() {
            Some("module") => Ok(()),
            Some(value) => Err(format!("type is {value:?}, not \"module\"")),
            None => Err("Missing \"type\": \"module\"".to_owned()),
        }
    }

    /// Whether a script with the given name exists.
    pub(crate) fn has_script(&self, name: &str) -> bool {
        self.scripts.as_ref().is_some_and(|s| s.contains_key(name))
    }

    /// Whether Prettier takes its configuration from the `prettier` key, as it does when the key's value is truthy.
    pub(crate) fn configures_prettier(&self) -> bool {
        self.prettier.as_ref().is_some_and(|value| match value {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
            Value::String(value) => !value.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        })
    }

    /// The script with the given name, if it exists.
    pub(crate) fn script(&self, name: &str) -> Option<&str> {
        self.scripts.as_ref()?.get(name).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::Source;

    #[test]
    fn exact_specifiers_name_one_version() {
        for specifier in [
            "19.2.0",
            "1.0.0-rc.1",
            "npm:react@19.2.0",
            "npm:@scope/name@1.2.3",
            "jsr:@std/path@1.1.2",
            "jsr:1.1.2",
            "workspace:*",
            "workspace:^",
            "link:../local",
            "file:../local.tgz",
            "catalog:",
            "git+https://example.com/repo.git#main",
            "https://example.com/package.tgz",
            "github:owner/repo",
            "owner/repo#main",
            "owner/repo#semver:^1",
            "github:owner/repo#semver:^1",
            "git+ssh://git@github.com:owner/repo.git#v1",
            "git@github.com:owner/repo.git",
            "../local",
            "./local",
            ".",
            "..",
            "~/local",
            "..\\local",
            "C:\\local",
            "local.tgz",
            "local.TAR.GZ",
            "local.tar",
            "NPM:react@19.2.0",
        ] {
            assert_eq!(check_specifier_exact(specifier), Ok(()), "{specifier}");
        }

        for specifier in [
            "^19.2.0",
            "~19.2.0",
            "19.2",
            "19",
            ">=19.2.0",
            "19.2.0 || 20.0.0",
            "19.2.x",
            "*",
            "",
            "latest",
            "=19.2.0",
            "v19.2.0",
            "19.2.0+build",
            "npm:react@^19",
            "npm:react",
            "npm:@scope/name",
            "jsr:^1.1.2",
            "jsr:@std/path",
            "npm:react@latest",
            "NPM:react@^19",
            "Jsr:^1.1.2",
            "1.2.3 - 1.2.4",
        ] {
            assert!(check_specifier_exact(specifier).is_err(), "{specifier}");
        }
    }

    #[test]
    fn lists_unpinned_dependencies_in_order() {
        let config = parse(
            r#"{
                "dependencies": { "react": "^19.2.0", "local": "workspace:*", "pinned": "1.0.0" },
                "devDependencies": { "vitest": "5.0.5", "eslint": "~10.14", "@types/node": "latest" },
                "optionalDependencies": { "fsevents": "2" },
                "peerDependencies": { "react": ">=19" }
            }"#,
        );

        assert_eq!(
            config.check_dependencies_exact(),
            Err(r#"Not pinned with x.y.z: dependencies.react ("^19.2.0"), devDependencies.@types/node ("latest"), devDependencies.eslint ("~10.14"), optionalDependencies.fsevents ("2")"#.to_owned())
        );
        assert_eq!(parse("{}").check_dependencies_exact(), Ok(()));
    }

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
    fn rejects_corepacks_package_manager() {
        for json in ["{}", r#"{ "packageManager": null }"#] {
            assert_eq!(parse(json).check_no_package_manager(), Ok(()), "{json}");
        }

        for (json, value) in [
            (r#"{ "packageManager": "pnpm@12.7.0" }"#, r#""pnpm@12.7.0""#),
            (
                r#"{ "packageManager": { "name": "pnpm" } }"#,
                r#"{"name":"pnpm"}"#,
            ),
        ] {
            assert_eq!(
                parse(json).check_no_package_manager(),
                Err(format!(
                    "Sets Corepack's packageManager ({value}); pin pnpm only in devEngines.packageManager"
                )),
                "{json}"
            );
        }
    }

    #[test]
    fn requires_the_release_version() {
        let release = Release::Version {
            version: semver::Version::new(0, 2, 0),
            source: Source::Changelog,
        };
        assert_eq!(
            parse(r#"{ "version": "0.2.0" }"#).check_version(&release),
            Ok(())
        );
        assert_eq!(
            parse(r#"{ "version": "0.1.0" }"#).check_version(&release),
            Err("version is 0.1.0, not 0.2.0, the latest release in CHANGELOG.md".to_owned())
        );
        assert_eq!(
            parse(r#"{ "version": 1 }"#).check_version(&release),
            Err("version is 1, not a string".to_owned())
        );
        assert_eq!(
            parse("{}").check_version(&release),
            Err(
                "Missing version, which should be 0.2.0, the latest release in CHANGELOG.md"
                    .to_owned()
            )
        );
    }

    #[test]
    fn requires_private_true() {
        assert_eq!(parse(r#"{ "private": true }"#).check_private(), Ok(()));
        for json in ["{}", r#"{ "private": null }"#] {
            assert_eq!(
                parse(json).check_private(),
                Err("Missing \"private\": true".to_owned()),
                "{json}"
            );
        }
        for (json, value) in [
            (r#"{ "private": false }"#, "false"),
            (r#"{ "private": "true" }"#, r#""true""#),
        ] {
            assert_eq!(
                parse(json).check_private(),
                Err(format!("private is {value}, not true")),
                "{json}"
            );
        }
    }

    #[test]
    fn requires_module_type() {
        assert_eq!(parse(r#"{ "type": "module" }"#).check_module_type(), Ok(()));
        assert_eq!(
            parse("{}").check_module_type(),
            Err("Missing \"type\": \"module\"".to_owned())
        );
        assert_eq!(
            parse(r#"{ "type": "commonjs" }"#).check_module_type(),
            Err(r#"type is "commonjs", not "module""#.to_owned())
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

    #[test]
    fn prettier_key_counts_when_truthy() {
        let configures = |json: &str| {
            serde_json::from_str::<PackageJson>(json)
                .unwrap()
                .configures_prettier()
        };
        for json in [
            r#"{"prettier": "@acme/prettier-config"}"#,
            r#"{"prettier": {}}"#,
            r#"{"prettier": true}"#,
            r#"{"prettier": 1}"#,
        ] {
            assert!(configures(json), "{json}");
        }
        for json in [
            "{}",
            r#"{"prettier": null}"#,
            r#"{"prettier": false}"#,
            r#"{"prettier": 0}"#,
            r#"{"prettier": ""}"#,
        ] {
            assert!(!configures(json), "{json}");
        }
    }
}

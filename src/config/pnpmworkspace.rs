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

    /// The dependencies allowed (`true`) or forbidden (`false`) to run build scripts, by package name pattern. Kept
    /// unparsed, so that one of the wrong shape fails only its check.
    pub allow_builds: Option<Value>,

    /// Whether every dependency that `allowBuilds` doesn't deny may run build scripts.
    pub dangerously_allow_all_builds: Option<Value>,

    /// Whether a dependency running build scripts without being listed in `allowBuilds` fails the install, as it does
    /// by default.
    pub strict_dep_builds: Option<Value>,

    /// The settings pnpm 11 replaced with `allowBuilds`, which it no longer reads.
    pub only_built_dependencies: Option<Value>,
    pub only_built_dependencies_file: Option<Value>,
    pub never_built_dependencies: Option<Value>,
    pub ignored_built_dependencies: Option<Value>,
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

    /// Check that `allowBuilds` lists the dependencies allowed to run build scripts: a map, possibly empty, from
    /// package name patterns to `true` or `false`.
    pub(crate) fn check_allow_builds(&self) -> Result<(), String> {
        let value = self.allow_builds.as_ref().ok_or("Missing allowBuilds")?;
        let Value::Object(entries) = value else {
            return Err(format!(
                "allowBuilds {value} isn't a map of package names to true or false"
            ));
        };
        let others: Vec<&str> = entries
            .iter()
            .filter(|(_, allowed)| !allowed.is_boolean())
            .map(|(name, _)| name.as_str())
            .collect();
        if others.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "allowBuilds sets {} to something other than true or false",
                others.join(", ")
            ))
        }
    }

    /// Check that none of the build settings pnpm 11 removed in favor of `allowBuilds` is set, since pnpm ignores
    /// them.
    pub(crate) fn check_no_legacy_build_settings(&self) -> Result<(), String> {
        let set: Vec<&str> = [
            ("onlyBuiltDependencies", &self.only_built_dependencies),
            (
                "onlyBuiltDependenciesFile",
                &self.only_built_dependencies_file,
            ),
            ("neverBuiltDependencies", &self.never_built_dependencies),
            ("ignoredBuiltDependencies", &self.ignored_built_dependencies),
        ]
        .into_iter()
        .filter(|(_, value)| value.is_some())
        .map(|(name, _)| name)
        .collect();
        if set.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Sets {}, which pnpm 11 replaced with allowBuilds",
                set.join(", ")
            ))
        }
    }

    /// Check that nothing lets dependencies run build scripts that `allowBuilds` doesn't allow:
    /// `dangerouslyAllowAllBuilds: true` lets every dependency not denied there run them, and `strictDepBuilds: false` only warns about
    /// the unlisted ones.
    pub(crate) fn check_builds_restricted(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if self.dangerously_allow_all_builds == Some(Value::Bool(true)) {
            problems.push("dangerouslyAllowAllBuilds is true");
        }
        if self.strict_dep_builds == Some(Value::Bool(false)) {
            problems.push("strictDepBuilds is false");
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
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

    fn parse(yaml: &str) -> PnpmWorkspace {
        serde_saphyr::from_str(yaml).unwrap()
    }

    #[test]
    fn allow_builds_maps_packages_to_booleans() {
        for yaml in [
            "allowBuilds: {}\n",
            "allowBuilds:\n  esbuild: true\n  core-js: false\n  \"@scope/*\": true\n",
        ] {
            assert_eq!(parse(yaml).check_allow_builds(), Ok(()), "{yaml}");
        }
        assert_eq!(
            parse("minimumReleaseAge: 4320\n").check_allow_builds(),
            Err("Missing allowBuilds".to_owned())
        );
        assert_eq!(
            parse("allowBuilds: [esbuild]\n").check_allow_builds(),
            Err(
                "allowBuilds [\"esbuild\"] isn't a map of package names to true or false"
                    .to_owned()
            )
        );
        assert_eq!(
            parse("allowBuilds:\n  core-js: false\n  esbuild: \"true\"\n  sharp:\n")
                .check_allow_builds(),
            Err("allowBuilds sets esbuild, sharp to something other than true or false".to_owned())
        );
    }

    #[test]
    fn rejects_the_removed_build_settings() {
        assert_eq!(
            parse("allowBuilds: {}\n").check_no_legacy_build_settings(),
            Ok(())
        );
        assert_eq!(
            parse("onlyBuiltDependencies: [esbuild]\nignoredBuiltDependencies: []\n")
                .check_no_legacy_build_settings(),
            Err(
                "Sets onlyBuiltDependencies, ignoredBuiltDependencies, which pnpm 11 replaced with allowBuilds"
                    .to_owned()
            )
        );
        for setting in [
            "onlyBuiltDependencies",
            "onlyBuiltDependenciesFile",
            "neverBuiltDependencies",
            "ignoredBuiltDependencies",
        ] {
            assert_eq!(
                parse(&format!("{setting}: []\n")).check_no_legacy_build_settings(),
                Err(format!(
                    "Sets {setting}, which pnpm 11 replaced with allowBuilds"
                )),
                "{setting}"
            );
        }
    }

    #[test]
    fn builds_stay_restricted_to_allow_builds() {
        for yaml in [
            "allowBuilds: {}\n",
            "dangerouslyAllowAllBuilds: false\nstrictDepBuilds: true\n",
        ] {
            assert_eq!(parse(yaml).check_builds_restricted(), Ok(()), "{yaml}");
        }
        assert_eq!(
            parse("dangerouslyAllowAllBuilds: true\n").check_builds_restricted(),
            Err("dangerouslyAllowAllBuilds is true".to_owned())
        );
        assert_eq!(
            parse("dangerouslyAllowAllBuilds: true\nstrictDepBuilds: false\n")
                .check_builds_restricted(),
            Err("dangerouslyAllowAllBuilds is true; strictDepBuilds is false".to_owned())
        );
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

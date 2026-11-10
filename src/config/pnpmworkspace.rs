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

    /// What each dependency may do, by package name pattern: whether it may run build scripts (`build`) and which of
    /// its agent skills pnpm links (`skills`). Kept unparsed, so that one of the wrong shape fails only its check.
    pub permissions: Option<Value>,

    /// Whether every dependency that `permissions` doesn't deny may run build scripts.
    pub dangerously_allow_all_builds: Option<Value>,

    /// Whether a dependency running build scripts without being approved or denied in `permissions` fails the
    /// install, as it does by default.
    pub strict_dep_builds: Option<Value>,

    /// The older map of dependencies allowed to run build scripts, still read for those `permissions` doesn't list.
    pub allow_builds: Option<Value>,

    /// Whether pnpm refuses a version published with weaker trust evidence, such as provenance, than an earlier one.
    pub trust_policy: Option<Value>,

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

    /// Check that `permissions` records what dependencies may do: a map, possibly empty, from package name patterns
    /// to capabilities. pnpm validates the entries itself, but a project without `permissions` has its build
    /// approvals written to `allowBuilds` instead.
    pub(crate) fn check_permissions(&self) -> Result<(), String> {
        let value = self.permissions.as_ref().ok_or("Missing permissions")?;
        if value.is_object() {
            Ok(())
        } else {
            Err(format!(
                "permissions {value} isn't a map of package names to capabilities"
            ))
        }
    }

    /// Check that none of the older build settings is set: `allowBuilds`, which pnpm still reads beside `permissions`,
    /// and those pnpm 11 removed in favor of `allowBuilds`, which pnpm ignores.
    pub(crate) fn check_no_legacy_build_settings(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if self.allow_builds.is_some() {
            problems.push("allowBuilds, which permissions replaces".to_owned());
        }
        let removed: Vec<&str> = [
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
        if !removed.is_empty() {
            problems.push(format!("{}, which pnpm 11 removed", removed.join(", ")));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("Sets {}", problems.join("; ")))
        }
    }

    /// Check that `trustPolicy` is `no-downgrade`. pnpm's default, `off`, installs a version published with weaker
    /// trust evidence than an earlier one, as a compromised release often is.
    pub(crate) fn check_trust_policy(&self) -> Result<(), String> {
        match &self.trust_policy {
            Some(Value::String(policy)) if policy == "no-downgrade" => Ok(()),
            Some(value) => Err(format!("trustPolicy is {value}, not \"no-downgrade\"")),
            None => Err(
                "Missing trustPolicy, so pnpm installs a version with weaker trust evidence"
                    .to_owned(),
            ),
        }
    }

    /// Check that nothing lets dependencies run build scripts that `permissions` doesn't allow:
    /// `dangerouslyAllowAllBuilds: true` lets every dependency not denied there run them, and `strictDepBuilds: false`
    /// only warns about the unreviewed ones.
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
            "minimumReleaseAge: 4320\npermissions: {}\n",
        ] {
            assert_eq!(check(yaml), Ok(()), "{yaml}");
        }
    }

    fn parse(yaml: &str) -> PnpmWorkspace {
        serde_saphyr::from_str(yaml).unwrap()
    }

    #[test]
    fn permissions_map_packages_to_capabilities() {
        for yaml in [
            "permissions: {}\n",
            "permissions:\n  esbuild:\n    build: true\n  core-js:\n    build: false\n    skills: [guide]\n",
        ] {
            assert_eq!(parse(yaml).check_permissions(), Ok(()), "{yaml}");
        }
        assert_eq!(
            parse("minimumReleaseAge: 4320\n").check_permissions(),
            Err("Missing permissions".to_owned())
        );
        assert_eq!(
            parse("allowBuilds: {}\n").check_permissions(),
            Err("Missing permissions".to_owned())
        );
        assert_eq!(
            parse("permissions: [esbuild]\n").check_permissions(),
            Err(
                "permissions [\"esbuild\"] isn't a map of package names to capabilities".to_owned()
            )
        );
    }

    #[test]
    fn rejects_the_older_build_settings() {
        assert_eq!(
            parse("permissions: {}\n").check_no_legacy_build_settings(),
            Ok(())
        );
        assert_eq!(
            parse("allowBuilds: {}\npermissions: {}\n").check_no_legacy_build_settings(),
            Err("Sets allowBuilds, which permissions replaces".to_owned())
        );
        assert_eq!(
            parse("onlyBuiltDependencies: [esbuild]\nignoredBuiltDependencies: []\n")
                .check_no_legacy_build_settings(),
            Err(
                "Sets onlyBuiltDependencies, ignoredBuiltDependencies, which pnpm 11 removed"
                    .to_owned()
            )
        );
        assert_eq!(
            parse("allowBuilds:\n  esbuild: true\nneverBuiltDependencies: []\n")
                .check_no_legacy_build_settings(),
            Err(
                "Sets allowBuilds, which permissions replaces; neverBuiltDependencies, which pnpm 11 removed"
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
                Err(format!("Sets {setting}, which pnpm 11 removed")),
                "{setting}"
            );
        }
    }

    #[test]
    fn builds_stay_restricted_to_permissions() {
        for yaml in [
            "permissions: {}\n",
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
    fn trust_may_not_be_downgraded() {
        assert_eq!(
            parse("trustPolicy: no-downgrade\n").check_trust_policy(),
            Ok(())
        );
        assert_eq!(
            parse("trustPolicy: \"off\"\n").check_trust_policy(),
            Err("trustPolicy is \"off\", not \"no-downgrade\"".to_owned())
        );
        assert_eq!(
            parse("permissions: {}\n").check_trust_policy(),
            Err(
                "Missing trustPolicy, so pnpm installs a version with weaker trust evidence"
                    .to_owned()
            )
        );
    }

    #[test]
    fn rejects_a_missing_or_other_age() {
        assert_eq!(
            check("permissions: {}\n"),
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

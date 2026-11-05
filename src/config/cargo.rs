// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `Cargo.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::{BTreeMap, HashMap};

use semver::{Op, VersionReq};
use serde::Deserialize;

/// Top-level `Cargo.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoToml {
    pub package: Option<CargoPackage>,
    pub lints: Option<CargoLints>,
    pub workspace: Option<CargoWorkspace>,
    /// Explicit `[[bin]]` targets.
    pub bin: Vec<CargoBinTarget>,
    #[serde(flatten)]
    pub dependencies: CargoDependencyTables,
    /// Platform-specific dependencies, keyed by the target's `cfg` expression or triple.
    pub target: BTreeMap<String, CargoDependencyTables>,
}

/// The dependency tables of a package or of one of its targets.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoDependencyTables {
    pub dependencies: BTreeMap<String, CargoDependency>,
    /// Editions before 2024 also accept the names with underscores.
    #[serde(alias = "dev_dependencies")]
    pub dev_dependencies: BTreeMap<String, CargoDependency>,
    #[serde(alias = "build_dependencies")]
    pub build_dependencies: BTreeMap<String, CargoDependency>,
}

impl CargoDependencyTables {
    /// Each table's name and dependencies.
    const fn tables(&self) -> [(&'static str, &BTreeMap<String, CargoDependency>); 3] {
        [
            ("dependencies", &self.dependencies),
            ("dev-dependencies", &self.dev_dependencies),
            ("build-dependencies", &self.build_dependencies),
        ]
    }
}

/// One dependency: a version requirement, or a table that may give one.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum CargoDependency {
    Version(String),
    Detailed(CargoDependencyDetail),
    /// Anything else, such as a table with a field of the wrong type, which Cargo would reject. Kept, so that it fails
    /// only the check reading it.
    Other(toml::Value),
}

/// A dependency given as a table. Only the fields that say where it comes from are modeled.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoDependencyDetail {
    pub version: Option<String>,
    pub path: Option<toml::Value>,
    pub git: Option<toml::Value>,
    pub workspace: Option<toml::Value>,
}

impl CargoDependency {
    /// Check that this dependency is pinned to an exact version (`=x.y.z`).
    ///
    /// Dependencies from a path or a Git repository have no version to pin, and one inherited from the workspace is
    /// checked where `[workspace.dependencies]` declares it, so these pass.
    pub(crate) fn check_exact(&self) -> Result<(), String> {
        let requirement = match self {
            Self::Detailed(detail)
                if detail.path.is_some() || detail.git.is_some() || detail.workspace.is_some() =>
            {
                return Ok(());
            }
            Self::Version(requirement)
            | Self::Detailed(CargoDependencyDetail {
                version: Some(requirement),
                ..
            }) => requirement,
            Self::Detailed(_) => return Err("no version".to_owned()),
            Self::Other(value) => return Err(format!("{value} isn't a valid dependency")),
        };

        if is_exact_requirement(requirement) {
            Ok(())
        } else {
            Err(format!("\"{requirement}\""))
        }
    }
}

/// Whether a Cargo version requirement allows exactly one version: a single `=` comparator naming the major, minor and
/// patch versions.
fn is_exact_requirement(requirement: &str) -> bool {
    VersionReq::parse(requirement).is_ok_and(|req| {
        matches!(
            req.comparators.as_slice(),
            [comparator] if comparator.op == Op::Exact
                && comparator.minor.is_some()
                && comparator.patch.is_some()
        )
    })
}

/// The `[package]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoPackage {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    pub repository: Option<String>,
    /// Whether Cargo discovers binaries in `src/main.rs` and `src/bin/` (default `true`).
    pub autobins: Option<bool>,
}

/// The `[lints]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoLints {
    pub clippy: Option<HashMap<String, toml::Value>>,
    pub rust: Option<HashMap<String, toml::Value>>,
}

/// The `[workspace]` table (subset).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoWorkspace {
    pub lints: Option<CargoLints>,
    /// Dependencies that members inherit with `workspace = true`.
    pub dependencies: BTreeMap<String, CargoDependency>,
}

/// A `[[bin]]` target entry.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoBinTarget {
    pub name: Option<String>,
}

impl CargoToml {
    /// Whether the project defines any binary targets (explicit `[[bin]]` or inferred from `src/main.rs`).
    ///
    /// Note: the caller must separately check for `src/main.rs` on the filesystem.
    pub(crate) const fn has_explicit_bin_targets(&self) -> bool {
        !self.bin.is_empty()
    }

    /// Check that every dependency this manifest declares is pinned exactly, including those of each target and of
    /// `[workspace.dependencies]`.
    ///
    /// The error lists each dependency that isn't, by its table and name, with the reason.
    pub(crate) fn check_dependencies_exact(&self) -> Result<(), String> {
        let package = self
            .dependencies
            .tables()
            .into_iter()
            .map(|(table, deps)| (table.to_owned(), deps));
        let targets = self.target.iter().flat_map(|(target, tables)| {
            tables
                .tables()
                .into_iter()
                .map(move |(table, deps)| (format!("target.'{target}'.{table}"), deps))
        });
        let workspace = self
            .workspace
            .iter()
            .map(|workspace| ("workspace.dependencies".to_owned(), &workspace.dependencies));

        let unpinned: Vec<String> = package
            .chain(targets)
            .chain(workspace)
            .flat_map(|(table, deps)| {
                deps.iter().filter_map(move |(name, dep)| {
                    dep.check_exact()
                        .err()
                        .map(|reason| format!("{table}.{name} ({reason})"))
                })
            })
            .collect();

        if unpinned.is_empty() {
            Ok(())
        } else {
            Err(format!("Not pinned with =x.y.z: {}", unpinned.join(", ")))
        }
    }

    /// Get the effective Clippy lint configuration, preferring package-level over workspace-level.
    pub(crate) fn effective_clippy_lints(&self) -> Option<&HashMap<String, toml::Value>> {
        self.lints
            .as_ref()
            .and_then(|l| l.clippy.as_ref())
            .or_else(|| self.workspace.as_ref()?.lints.as_ref()?.clippy.as_ref())
    }

    /// Check whether a specific Clippy lint group or lint is configured.
    pub(crate) fn has_clippy_lint(&self, name: &str) -> bool {
        self.effective_clippy_lints()
            .is_some_and(|lints| lints.contains_key(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(manifest: &str) -> CargoToml {
        toml::from_str(manifest).unwrap()
    }

    #[test]
    fn exact_requirements_name_one_full_version() {
        for requirement in ["=1.0.228", "= 1.0.228", "=0.0.1", "=1.0.0-rc.1"] {
            assert!(is_exact_requirement(requirement), "{requirement}");
        }
        for requirement in [
            "1.0.228",
            "^1.0.228",
            "~1.0.228",
            "=1.0",
            "=1",
            ">=1.0.0",
            "=1.0.0, <2",
            "*",
            "",
            "=1.0.x",
        ] {
            assert!(!is_exact_requirement(requirement), "{requirement}");
        }
    }

    #[test]
    fn pinned_manifest_passes() {
        let manifest = parse(
            r#"
            [dependencies]
            serde = { version = "=1.0.229", features = ["derive"] }
            local = { path = "../local" }
            fork = { git = "https://example.com/fork.git", branch = "main" }
            inherited = { workspace = true }

            [dev-dependencies]
            tempfile = "=3.27.0"
            both = { version = "1.0", path = "../both" }

            [target.'cfg(unix)'.dependencies]
            libc = "=0.2.177"

            [workspace.dependencies]
            inherited = "=1.2.3"
            "#,
        );

        assert_eq!(manifest.check_dependencies_exact(), Ok(()));
    }

    #[test]
    fn lists_every_unpinned_dependency() {
        let manifest = parse(
            r#"
            [dependencies]
            serde = "1.0"
            anyhow = "=1.0.104"
            unversioned = { features = ["std"] }

            [build-dependencies]
            cc = { version = "^1.2" }

            [target.'cfg(windows)'.dev-dependencies]
            windows = "0.62"

            [workspace.dependencies]
            regex = "~1.13"
            "#,
        );

        assert_eq!(
            manifest.check_dependencies_exact(),
            Err("Not pinned with =x.y.z: dependencies.serde (\"1.0\"), dependencies.unversioned (no version), \
                 build-dependencies.cc (\"^1.2\"), target.'cfg(windows)'.dev-dependencies.windows (\"0.62\"), \
                 workspace.dependencies.regex (\"~1.13\")"
                .to_owned())
        );
    }

    #[test]
    fn malformed_dependency_fails_only_the_check() {
        let manifest =
            parse("[package]\nname = \"x\"\n\n[dependencies]\nodd = 5\ntypo = { version = 1 }\n");

        assert!(manifest.package.is_some());
        assert_eq!(
            manifest.check_dependencies_exact(),
            Err("Not pinned with =x.y.z: dependencies.odd (5 isn't a valid dependency), dependencies.typo \
                 ({ version = 1 } isn't a valid dependency)"
                .to_owned())
        );
    }

    #[test]
    fn accepts_the_older_table_names() {
        let manifest = parse(
            r#"
            [dev_dependencies]
            a = "1"

            [target.'cfg(unix)'.build_dependencies]
            b = "2"
            "#,
        );

        assert_eq!(
            manifest.check_dependencies_exact(),
            Err("Not pinned with =x.y.z: dev-dependencies.a (\"1\"), target.'cfg(unix)'.build-dependencies.b (\"2\")"
                .to_owned())
        );
    }
}

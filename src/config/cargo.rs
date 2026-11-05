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
    pub description: Option<Inheritable<String>>,
    pub license: Option<Inheritable<String>>,
    pub edition: Option<Inheritable<String>>,
    pub rust_version: Option<Inheritable<String>>,
    pub repository: Option<Inheritable<String>>,
    /// Whether Cargo discovers binaries in `src/main.rs` and `src/bin/` (default `true`).
    pub autobins: Option<bool>,
}

/// A `[package]` field that may be inherited from the workspace's `[workspace.package]`, with `field.workspace = true`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Inheritable<T> {
    Value(T),
    Workspace {
        /// Cargo rejects `false`, so it is treated as if the field weren't set.
        workspace: bool,
    },
}

impl<T> Inheritable<T> {
    /// The field's value: its own, or, if inherited, the one `inherited` gives.
    const fn resolve<'a>(&'a self, inherited: Option<&'a T>) -> Option<&'a T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Workspace { workspace: true } => inherited,
            Self::Workspace { workspace: false } => None,
        }
    }
}

/// The `[workspace.package]` table: the fields members may inherit.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct CargoWorkspacePackage {
    pub description: Option<String>,
    pub license: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    pub repository: Option<String>,
}

/// A package's metadata, with the fields it inherits from its workspace resolved. A field is `None` if it isn't set,
/// or if it is inherited and the workspace doesn't set it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PackageMetadata<'a> {
    pub description: Option<&'a str>,
    pub license: Option<&'a str>,
    pub edition: Option<&'a str>,
    pub rust_version: Option<&'a str>,
    pub repository: Option<&'a str>,
}

/// The `[lints]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoLints {
    pub clippy: Option<HashMap<String, toml::Value>>,
    pub rust: Option<HashMap<String, toml::Value>>,
    /// Whether the package inherits the workspace's `[workspace.lints]` (`[lints] workspace = true`).
    pub workspace: bool,
}

/// The `[workspace]` table (subset).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoWorkspace {
    /// The fields members may inherit.
    pub package: Option<CargoWorkspacePackage>,
    /// The lints members may inherit.
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

    /// The package's metadata, with the fields it inherits resolved from `workspace`, the `[workspace]` table of its
    /// workspace's root manifest. A manifest without a `[package]` has no metadata.
    pub(crate) fn metadata<'a>(
        &'a self,
        workspace: Option<&'a CargoWorkspace>,
    ) -> PackageMetadata<'a> {
        let Some(package) = &self.package else {
            return PackageMetadata::default();
        };
        let inherited = workspace.and_then(|workspace| workspace.package.as_ref());

        let resolve =
            |field: &'a Option<Inheritable<String>>,
             from_workspace: fn(&CargoWorkspacePackage) -> &Option<String>| {
                field
                    .as_ref()?
                    .resolve(inherited.and_then(|shared| from_workspace(shared).as_ref()))
                    .map(String::as_str)
            };

        PackageMetadata {
            description: resolve(&package.description, |p| &p.description),
            license: resolve(&package.license, |p| &p.license),
            edition: resolve(&package.edition, |p| &p.edition),
            rust_version: resolve(&package.rust_version, |p| &p.rust_version),
            repository: resolve(&package.repository, |p| &p.repository),
        }
    }

    /// The Clippy lints that apply, resolved against `workspace`, the `[workspace]` table of the workspace's root
    /// manifest.
    ///
    /// A package's lints are its own `[lints.clippy]`, or the workspace's `[workspace.lints.clippy]` if it inherits
    /// them with `[lints] workspace = true`; Cargo doesn't apply the workspace's lints otherwise, even to the package in
    /// the root manifest. A virtual manifest, which has no package, is checked for the lints its members can inherit.
    /// The error says why there are none.
    pub(crate) fn clippy_lints<'a>(
        &'a self,
        workspace: Option<&'a CargoWorkspace>,
    ) -> Result<&'a HashMap<String, toml::Value>, String> {
        let workspace_lints = workspace
            .and_then(|workspace| workspace.lints.as_ref())
            .and_then(|lints| lints.clippy.as_ref());

        if self.package.is_none() {
            return workspace_lints.ok_or_else(|| "No [workspace.lints.clippy] section".to_owned());
        }

        match &self.lints {
            Some(CargoLints { workspace: true, .. }) => workspace_lints.ok_or_else(|| {
                "Inherits the workspace's lints, but there is no [workspace.lints.clippy] section".to_owned()
            }),
            Some(CargoLints {
                clippy: Some(lints),
                ..
            }) => Ok(lints),
            _ if workspace_lints.is_some() => Err(
                "No [lints.clippy] section, and [workspace.lints.clippy] isn't inherited ([lints] workspace = true)"
                    .to_owned(),
            ),
            _ => Err("No [lints.clippy] section".to_owned()),
        }
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

    #[test]
    fn resolves_inherited_metadata() {
        let manifest = parse(
            r#"
            [package]
            name = "x"
            description = "Own."
            license.workspace = true
            edition = { workspace = true }
            rust-version.workspace = true
            repository = { workspace = false }

            [workspace.package]
            description = "Inherited."
            license = "MIT"
            edition = "2024"
            repository = "https://example.com/x"
            "#,
        );

        assert_eq!(
            manifest.metadata(manifest.workspace.as_ref()),
            PackageMetadata {
                description: Some("Own."),
                license: Some("MIT"),
                edition: Some("2024"),
                rust_version: None,
                repository: None,
            }
        );
        assert_eq!(
            manifest.metadata(None),
            PackageMetadata {
                description: Some("Own."),
                ..PackageMetadata::default()
            }
        );
    }

    #[test]
    fn virtual_manifest_has_no_metadata() {
        let manifest = parse("[workspace.package]\nlicense = \"MIT\"\n");

        assert_eq!(
            manifest.metadata(manifest.workspace.as_ref()),
            PackageMetadata::default()
        );
    }

    #[test]
    fn workspace_lints_apply_only_when_inherited() {
        let lints = |manifest: &str| {
            let manifest = parse(manifest);
            manifest
                .clippy_lints(manifest.workspace.as_ref())
                .map(|lints| lints.keys().cloned().collect::<Vec<_>>())
        };
        let workspace = "[workspace.lints.clippy]\nnursery = \"warn\"\n";

        assert_eq!(
            lints("[package]\nname = \"x\"\n\n[lints.clippy]\npedantic = \"warn\"\n"),
            Ok(vec!["pedantic".to_owned()])
        );
        assert_eq!(
            lints(&format!(
                "[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n\n{workspace}"
            )),
            Ok(vec!["nursery".to_owned()])
        );
        assert_eq!(lints(workspace), Ok(vec!["nursery".to_owned()]));

        assert!(lints(&format!("[package]\nname = \"x\"\n\n{workspace}")).is_err());
        assert!(lints("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n").is_err());
        assert!(lints("[package]\nname = \"x\"\n").is_err());
        assert!(lints("[workspace]\n").is_err());
    }
}

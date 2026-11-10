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
    /// The path of a dependency on a local package, relative to the manifest declaring it.
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Detailed(detail) => detail.path.as_ref()?.as_str(),
            Self::Version(_) | Self::Other(_) => None,
        }
    }

    /// Whether the dependency is inherited from the workspace's `[workspace.dependencies]` (`workspace = true`).
    pub(crate) fn is_inherited(&self) -> bool {
        matches!(self, Self::Detailed(detail) if detail.workspace.as_ref().and_then(toml::Value::as_bool) == Some(true))
    }

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
    /// Whether `cargo publish` may publish the package, and to which registries (default: any).
    pub publish: Option<Inheritable<CargoPublish>>,
}

/// The `publish` field: whether the package may be published at all, or the registries it may be published to.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum CargoPublish {
    Allowed(bool),
    Registries(Vec<String>),
}

impl CargoPublish {
    /// Whether the package may be published anywhere: `true`, or a non-empty list of registries.
    const fn allowed(&self) -> bool {
        match self {
            Self::Allowed(allowed) => *allowed,
            Self::Registries(registries) => !registries.is_empty(),
        }
    }
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
    pub publish: Option<CargoPublish>,
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

impl CargoLints {
    /// The lints of `tool`: `clippy`, or `rust` for the compiler's own.
    fn tool(&self, tool: &str) -> Option<&HashMap<String, toml::Value>> {
        match tool {
            "clippy" => self.clippy.as_ref(),
            "rust" => self.rust.as_ref(),
            _ => None,
        }
    }
}

/// The `[workspace]` table (subset).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct CargoWorkspace {
    /// The fields members may inherit.
    pub package: Option<CargoWorkspacePackage>,
    /// The lints members may inherit.
    pub lints: Option<CargoLints>,
    /// The members' directories, or glob patterns matching them, relative to the workspace root.
    pub members: Vec<String>,
    /// Directories whose packages aren't members, relative to the workspace root.
    pub exclude: Vec<String>,
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

    /// The package's dependency tables, including those of each target, by their names in the manifest. The
    /// workspace's `[workspace.dependencies]` isn't one of them.
    pub(crate) fn package_dependency_tables(
        &self,
    ) -> impl Iterator<Item = (String, &BTreeMap<String, CargoDependency>)> {
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

        package.chain(targets)
    }

    /// Check that every dependency this manifest declares is pinned exactly, including those of each target and of
    /// `[workspace.dependencies]`.
    ///
    /// The error lists each dependency that isn't, by its table and name, with the reason.
    pub(crate) fn check_dependencies_exact(&self) -> Result<(), String> {
        let workspace = self
            .workspace
            .iter()
            .map(|workspace| ("workspace.dependencies".to_owned(), &workspace.dependencies));

        let unpinned: Vec<String> = self
            .package_dependency_tables()
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

    /// Whether the manifest's package may be published, with an inherited `publish` resolved from `workspace`, the
    /// `[workspace]` table of its workspace's root manifest: unless `publish` is `false` or an empty list of
    /// registries. A manifest without a package publishes nothing.
    pub(crate) fn is_publishable(&self, workspace: Option<&CargoWorkspace>) -> bool {
        let Some(package) = &self.package else {
            return false;
        };
        let inherited = workspace
            .and_then(|workspace| workspace.package.as_ref())
            .and_then(|shared| shared.publish.as_ref());
        package
            .publish
            .as_ref()
            .and_then(|publish| publish.resolve(inherited))
            .is_none_or(CargoPublish::allowed)
    }

    /// The Clippy lints that apply, resolved against `workspace`. See [`lints`](Self::lints).
    pub(crate) fn clippy_lints<'a>(
        &'a self,
        workspace: Option<&'a CargoWorkspace>,
    ) -> Result<&'a HashMap<String, toml::Value>, String> {
        self.lints("clippy", workspace)
    }

    /// The lints of `tool` (`clippy`, or `rust` for the compiler's own) that apply, resolved against `workspace`, the
    /// `[workspace]` table of the workspace's root manifest.
    ///
    /// A package's lints are its own `[lints.<tool>]`, or the workspace's `[workspace.lints.<tool>]` if it inherits
    /// them with `[lints] workspace = true`; Cargo doesn't apply the workspace's lints otherwise, even to the package in
    /// the root manifest. A manifest without a package has no lints. The error says why there are none.
    pub(crate) fn lints<'a>(
        &'a self,
        tool: &str,
        workspace: Option<&'a CargoWorkspace>,
    ) -> Result<&'a HashMap<String, toml::Value>, String> {
        let workspace_lints = workspace
            .and_then(|workspace| workspace.lints.as_ref())
            .and_then(|lints| lints.tool(tool));

        if self.package.is_none() {
            return Err("No [package] section, so no lints apply".to_owned());
        }

        let own = self.lints.as_ref();
        if own.is_some_and(|lints| lints.workspace) {
            return workspace_lints.ok_or_else(|| {
                format!("Inherits the workspace's lints, but there is no [workspace.lints.{tool}] section")
            });
        }
        if let Some(lints) = own.and_then(|lints| lints.tool(tool)) {
            return Ok(lints);
        }
        Err(if workspace_lints.is_some() {
            format!(
                "No [lints.{tool}] section, and [workspace.lints.{tool}] isn't inherited ([lints] workspace = true)"
            )
        } else {
            format!("No [lints.{tool}] section")
        })
    }
}

/// A lint level, in the order of strictness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LintLevel {
    Allow,
    Warn,
    Deny,
    Forbid,
}

impl LintLevel {
    /// The level a `[lints]` entry sets: its value, as a string or a table's `level`. An entry that sets none, or one
    /// Cargo doesn't know, is none.
    pub(crate) fn of(value: &toml::Value) -> Option<Self> {
        let level = match value {
            toml::Value::String(level) => level.as_str(),
            toml::Value::Table(table) => table.get("level")?.as_str()?,
            toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
            | toml::Value::Array(_) => return None,
        };
        match level {
            "allow" => Some(Self::Allow),
            "warn" => Some(Self::Warn),
            "deny" => Some(Self::Deny),
            "forbid" => Some(Self::Forbid),
            _ => None,
        }
    }

    /// The level's name, as the `[lints]` table spells it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Deny => "deny",
            Self::Forbid => "forbid",
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
    fn publish_resolves_inherited_values() {
        let workspace = |publish: &str| parse(&format!("[workspace.package]\n{publish}\n"));
        let publishable = |package: &str, shared: &CargoToml| {
            parse(&format!("[package]\nname = \"demo\"\n{package}\n"))
                .is_publishable(shared.workspace.as_ref())
        };
        let none = workspace("");

        for package in [
            "",
            "publish = true",
            "publish = [\"internal\"]",
            "publish.workspace = true",
        ] {
            assert!(publishable(package, &none), "{package}");
        }
        for package in ["publish = false", "publish = []"] {
            assert!(!publishable(package, &none), "{package}");
        }
        assert!(!publishable(
            "publish.workspace = true",
            &workspace("publish = false")
        ));
        assert!(publishable(
            "publish.workspace = true",
            &workspace("publish = [\"internal\"]")
        ));
        // A package's own value wins over the workspace's, which it doesn't inherit.
        assert!(publishable("", &workspace("publish = false")));
        assert!(!parse("[workspace]\n").is_publishable(None));
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

        assert!(lints(&format!("[package]\nname = \"x\"\n\n{workspace}")).is_err());
        assert!(lints("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n").is_err());
        assert!(lints("[package]\nname = \"x\"\n").is_err());
        assert!(lints(workspace).is_err());
    }

    #[test]
    fn lint_levels_are_read_from_strings_and_tables() {
        let level = |value: &str| {
            let table: toml::Table = toml::from_str(&format!("x = {value}")).unwrap();
            LintLevel::of(table.get("x").unwrap())
        };
        assert_eq!(level("\"warn\""), Some(LintLevel::Warn));
        assert_eq!(level("\"forbid\""), Some(LintLevel::Forbid));
        assert_eq!(
            level("{ level = \"deny\", priority = -1 }"),
            Some(LintLevel::Deny)
        );
        assert_eq!(level("{ priority = -1 }"), None);
        assert_eq!(level("\"loud\""), None);
        assert_eq!(level("1"), None);
        assert!(LintLevel::Allow < LintLevel::Warn && LintLevel::Deny < LintLevel::Forbid);
    }

    #[test]
    fn rust_lints_resolve_like_clippy_lints() {
        let lints = |manifest: &str| {
            let manifest = parse(manifest);
            manifest
                .lints("rust", manifest.workspace.as_ref())
                .map(|lints| lints.keys().cloned().collect::<Vec<_>>())
        };
        assert_eq!(
            lints("[package]\nname = \"x\"\n\n[lints.rust]\nunsafe_code = \"forbid\"\n"),
            Ok(vec!["unsafe_code".to_owned()])
        );
        assert_eq!(
            lints(
                "[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n"
            ),
            Ok(vec!["unsafe_code".to_owned()])
        );
        assert_eq!(
            lints("[package]\nname = \"x\"\n\n[lints.clippy]\npedantic = \"warn\"\n"),
            Err("No [lints.rust] section".to_owned())
        );
    }
}

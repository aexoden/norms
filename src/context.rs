// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Project context with lazy-parsed, cached config files.
//!
//! # Dependency Injection
//!
//! The [`ProjectContext`] is the primary mechanism for sharing parsed data between
//! checks. Instead of each check independently reading and parsing the same files,
//! the context parses each file at most once and caches the result.
//!
//! Any check that needs (for example) the parsed `devbox.json` simply calls
//! `ctx.devbox()`, which returns a [`ConfigStatus`] that the check can inspect
//! or pass to [`Report::require_parsed`](crate::report::Report::require_parsed).
//!
//! ```ignore
//! fn check_devbox(ctx: &ProjectContext, report: &mut Report) {
//!     // This call is free if another check already triggered the parse.
//!     let status = ctx.devbox();
//!     // Devbox is optional, so a project without it has nothing to check.
//!     if matches!(status, ConfigStatus::NotFound) {
//!         return;
//!     }
//!     let Some(config) = report.require_parsed("devbox", "devbox.json", status) else {
//!         return;
//!     };
//!     report.should(
//!         "devbox-packages",
//!         "devbox.json: has packages",
//!         !config.packages.is_empty(),
//!     );
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use pep508_rs::pep440_rs::Version;

use crate::cargo_workspace::{CargoWorkspaceMembers, find_members};
use crate::config::{
    CLIPPY_TOML, CargoConfig, CargoToml, ClippyToml, DenyToml, DevboxConfig, GIT_ARCHIVAL,
    HIDDEN_CLIPPY_TOML, MarkdownlintCli2Config, NormsConfig, PYTHON_VERSION, PackageJson,
    PnpmWorkspace, PrecommitConfig, PrettierConfig, PyprojectToml, RenovateConfig, ReuseToml,
    RustToolchain, TsConfig, Workflow, parse_python_version,
};
use crate::detection::Detection;
use crate::licenses::LicenseFiles;
use crate::models::{Component, Language};

/// The file name of markdownlint-cli2's configuration, at the root of the project.
pub(crate) const MARKDOWNLINT_CLI2_CONFIG: &str = ".markdownlint-cli2.jsonc";

/// The file name of the CI workflow in `.github/workflows/`.
pub(crate) const CI_WORKFLOW: &str = "ci.yaml";

/// The file name of the scheduled advisory audit workflow in `.github/workflows/`.
pub(crate) const AUDIT_WORKFLOW: &str = "audit.yaml";

/// The file that pins a Rust component's toolchain.
pub(crate) const RUST_TOOLCHAIN: &str = "rust-toolchain.toml";

/// rustup's legacy name for [`RUST_TOOLCHAIN`], which it still reads.
pub(crate) const LEGACY_RUST_TOOLCHAIN: &str = "rust-toolchain";

/// The result of attempting to read and parse a config file.
///
/// This three-state enum lets checks distinguish between "file missing",
/// "file present but malformed", and "file present and valid".
#[derive(Debug)]
pub(crate) enum ConfigStatus<T> {
    /// The file does not exist.
    NotFound,
    /// The file exists but could not be parsed.
    ParseError {
        /// A human-readable description of the parse error.
        error: String,
    },
    /// The file exists and was parsed successfully.
    Ok(T),
}

impl<T> ConfigStatus<T> {
    /// Get a reference to the parsed value, if available.
    pub(crate) const fn as_ref_ok(&self) -> Option<&T> {
        match self {
            Self::Ok(v) => Some(v),
            Self::NotFound | Self::ParseError { .. } => None,
        }
    }
}

/// Lazy-loading project context.
///
/// Each config file is parsed at most once using [`OnceLock`]. The first call to
/// a getter triggers the parse; subsequent calls return the cached result.
///
/// This holds the files that belong to the repository as a whole. Each component's own files, such as its
/// `Cargo.toml`, are in its [`ComponentContext`].
pub(crate) struct ProjectContext {
    path: PathBuf,
    detection: OnceLock<Detection>,
    component_contexts: OnceLock<Vec<ComponentContext>>,
    languages: OnceLock<BTreeSet<Language>>,
    cargo_config: OnceLock<ConfigStatus<CargoConfig>>,
    devbox: OnceLock<ConfigStatus<DevboxConfig>>,
    license_files: OnceLock<Arc<LicenseFiles>>,
    markdownlint_cli2: OnceLock<ConfigStatus<MarkdownlintCli2Config>>,
    norms: OnceLock<ConfigStatus<NormsConfig>>,
    renovate: OnceLock<ConfigStatus<RenovateConfig>>,
    reuse_toml: OnceLock<ConfigStatus<ReuseToml>>,
    precommit: OnceLock<ConfigStatus<PrecommitConfig>>,
    editorconfig: OnceLock<ConfigStatus<String>>,
    gitattributes: OnceLock<ConfigStatus<String>>,
    claude_md: OnceLock<ConfigStatus<String>>,
    workflows: OnceLock<BTreeMap<String, ConfigStatus<Workflow>>>,
    markdownlint_configs: OnceLock<Vec<PathBuf>>,
    release_tags: OnceLock<Vec<String>>,
}

impl ProjectContext {
    /// Create a new context for the project at the given path.
    pub(crate) const fn new(path: PathBuf) -> Self {
        Self {
            path,
            detection: OnceLock::new(),
            component_contexts: OnceLock::new(),
            languages: OnceLock::new(),
            cargo_config: OnceLock::new(),
            devbox: OnceLock::new(),
            license_files: OnceLock::new(),
            markdownlint_cli2: OnceLock::new(),
            norms: OnceLock::new(),
            renovate: OnceLock::new(),
            reuse_toml: OnceLock::new(),
            precommit: OnceLock::new(),
            editorconfig: OnceLock::new(),
            gitattributes: OnceLock::new(),
            claude_md: OnceLock::new(),
            workflows: OnceLock::new(),
            markdownlint_configs: OnceLock::new(),
            release_tags: OnceLock::new(),
        }
    }

    /// The root path of the project.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The project's release tags (cached): its git tags named `vX.Y.Z`. See [`crate::git::release_tags`].
    pub(crate) fn release_tags(&self) -> &[String] {
        self.release_tags
            .get_or_init(|| crate::git::release_tags(&self.path))
    }

    /// The search for the project's components (cached).
    ///
    /// The directories `norms.toml` excludes aren't searched; if `norms.toml` is malformed, none are excluded.
    pub(crate) fn detection(&self) -> &Detection {
        self.detection.get_or_init(|| {
            let excluded = self
                .norms()
                .as_ref_ok()
                .map(NormsConfig::excluded_paths)
                .unwrap_or_default();

            crate::detection::detect_components(&self.path, &excluded)
        })
    }

    /// Detected components (cached), in a stable order.
    pub(crate) fn components(&self) -> &[Component] {
        &self.detection().components
    }

    /// A context for each detected component (cached), in the order of [`components`](Self::components), so that
    /// every check of a component shares the files parsed for it.
    pub(crate) fn component_contexts(&self) -> &[ComponentContext] {
        self.component_contexts.get_or_init(|| {
            self.components()
                .iter()
                .map(|component| ComponentContext::new(self, component))
                .collect()
        })
    }

    /// The languages of the detected components (cached), in a stable order.
    pub(crate) fn languages(&self) -> &BTreeSet<Language> {
        self.languages.get_or_init(|| {
            self.components()
                .iter()
                .map(|component| component.language)
                .collect()
        })
    }

    /// Parsed `.cargo/config.toml` (cached).
    ///
    /// Cargo merges the config files of the directory it runs in and each of its parents, so the root's applies to
    /// every Rust component, wherever Cargo runs in the repository.
    pub(crate) fn cargo_config(&self) -> &ConfigStatus<CargoConfig> {
        self.cargo_config
            .get_or_init(|| parse_toml_file::<CargoConfig>(&self.path.join(".cargo/config.toml")))
    }

    /// Parsed `devbox.json` (cached). Uses JSON5 for flexibility.
    pub(crate) fn devbox(&self) -> &ConfigStatus<DevboxConfig> {
        self.devbox
            .get_or_init(|| parse_json5_file::<DevboxConfig>(&self.path.join("devbox.json")))
    }

    /// The license texts in `LICENSES/` and their copies at the root (cached).
    pub(crate) fn license_files(&self) -> &LicenseFiles {
        self.shared_license_files()
    }

    /// [`license_files`](Self::license_files), shared with each [`ComponentContext`].
    fn shared_license_files(&self) -> &Arc<LicenseFiles> {
        self.license_files
            .get_or_init(|| Arc::new(LicenseFiles::read(&self.path)))
    }

    /// Parsed `.markdownlint-cli2.jsonc` (cached).
    pub(crate) fn markdownlint_cli2(&self) -> &ConfigStatus<MarkdownlintCli2Config> {
        self.markdownlint_cli2.get_or_init(|| {
            parse_json5_file::<MarkdownlintCli2Config>(&self.path.join(MARKDOWNLINT_CLI2_CONFIG))
        })
    }

    /// The project's markdownlint configuration files (cached), by their paths relative to the root. See
    /// [`crate::detection::find_markdownlint_configs`].
    pub(crate) fn markdownlint_configs(&self) -> &[PathBuf] {
        self.markdownlint_configs
            .get_or_init(|| crate::detection::find_markdownlint_configs(&self.path))
    }

    /// Parsed `norms.toml` (cached).
    pub(crate) fn norms(&self) -> &ConfigStatus<NormsConfig> {
        self.norms
            .get_or_init(|| parse_toml_file::<NormsConfig>(&self.path.join("norms.toml")))
    }

    /// Parsed `renovate.json` (cached). Uses JSON5 for flexibility.
    pub(crate) fn renovate(&self) -> &ConfigStatus<RenovateConfig> {
        self.renovate
            .get_or_init(|| parse_json5_file::<RenovateConfig>(&self.path.join("renovate.json")))
    }

    /// Parsed `REUSE.toml` (cached).
    pub(crate) fn reuse_toml(&self) -> &ConfigStatus<ReuseToml> {
        self.reuse_toml
            .get_or_init(|| parse_toml_file::<ReuseToml>(&self.path.join("REUSE.toml")))
    }

    /// Parsed `.pre-commit-config.yaml` (cached).
    pub(crate) fn precommit(&self) -> &ConfigStatus<PrecommitConfig> {
        self.precommit.get_or_init(|| {
            parse_yaml_file::<PrecommitConfig>(&self.path.join(".pre-commit-config.yaml"))
        })
    }

    // Raw `.editorconfig` content (cached).
    pub(crate) fn editorconfig(&self) -> &ConfigStatus<String> {
        self.editorconfig
            .get_or_init(|| read_text_file(&self.path.join(".editorconfig")))
    }

    // Raw `.gitattributes` content (cached).
    pub(crate) fn gitattributes(&self) -> &ConfigStatus<String> {
        self.gitattributes
            .get_or_init(|| read_text_file(&self.path.join(".gitattributes")))
    }

    /// Raw `CLAUDE.md` content (cached).
    pub(crate) fn claude_md(&self) -> &ConfigStatus<String> {
        self.claude_md
            .get_or_init(|| read_text_file(&self.path.join("CLAUDE.md")))
    }

    /// Parsed workflow files in `.github/workflows/` (cached), keyed by file name.
    ///
    /// GitHub runs each `.yml` or `.yaml` file directly in that directory, so subdirectories and other files are
    /// skipped. Symlinks are followed. The map is empty if the directory doesn't exist or can't be read.
    pub(crate) fn workflows(&self) -> &BTreeMap<String, ConfigStatus<Workflow>> {
        self.workflows.get_or_init(|| {
            let Ok(entries) = fs::read_dir(self.path.join(".github/workflows")) else {
                return BTreeMap::new();
            };

            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let path = entry.path();
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "yml" || extension == "yaml")
                })
                .map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    (name, parse_yaml_file::<Workflow>(&entry.path()))
                })
                .collect()
        })
    }

    /// Parsed `.github/workflows/ci.yaml`, from [`workflows`](Self::workflows).
    pub(crate) fn ci_workflow(&self) -> &ConfigStatus<Workflow> {
        static NOT_FOUND: ConfigStatus<Workflow> = ConfigStatus::NotFound;

        self.workflows().get(CI_WORKFLOW).unwrap_or(&NOT_FOUND)
    }

    /// Parsed `.github/workflows/audit.yaml`, from [`workflows`](Self::workflows).
    pub(crate) fn audit_workflow(&self) -> &ConfigStatus<Workflow> {
        static NOT_FOUND: ConfigStatus<Workflow> = ConfigStatus::NotFound;

        self.workflows().get(AUDIT_WORKFLOW).unwrap_or(&NOT_FOUND)
    }
}

/// Lazy-loading context for one component of a project.
///
/// Like [`ProjectContext`], each config file is parsed at most once. Paths are relative to the component's directory,
/// which may be the project root or a subdirectory. A component has a single language, so only that language's
/// getters are ever called.
pub(crate) struct ComponentContext {
    path: PathBuf,
    root: PathBuf,
    component: Component,
    license_files: Arc<LicenseFiles>,
    cargo: OnceLock<ConfigStatus<CargoToml>>,
    cargo_members: OnceLock<CargoWorkspaceMembers>,
    cargo_config: OnceLock<ConfigStatus<CargoConfig>>,
    rust_toolchain: OnceLock<(String, ConfigStatus<RustToolchain>)>,
    clippy_toml: OnceLock<(String, ConfigStatus<ClippyToml>)>,
    member_clippy_tomls: OnceLock<Vec<(String, ConfigStatus<ClippyToml>)>>,
    deny_toml: OnceLock<ConfigStatus<DenyToml>>,
    pyproject: OnceLock<ConfigStatus<PyprojectToml>>,
    python_version: OnceLock<ConfigStatus<Version>>,
    git_archival: OnceLock<ConfigStatus<String>>,
    eslint_config: OnceLock<ConfigStatus<String>>,
    package_json: OnceLock<ConfigStatus<PackageJson>>,
    pnpm_workspace: OnceLock<ConfigStatus<PnpmWorkspace>>,
    prettier: OnceLock<ConfigStatus<PrettierConfig>>,
    tsconfig: OnceLock<ConfigStatus<TsConfig>>,
    typescript_referenced_configs: OnceLock<Vec<(String, ConfigStatus<TsConfig>)>>,
}

impl ComponentContext {
    /// Create a new context for the given component of `project`.
    pub(crate) fn new(project: &ProjectContext, component: &Component) -> Self {
        Self {
            path: project.path().join(&component.path),
            root: project.path().to_path_buf(),
            component: component.clone(),
            license_files: Arc::clone(project.shared_license_files()),
            cargo: OnceLock::new(),
            cargo_members: OnceLock::new(),
            cargo_config: OnceLock::new(),
            rust_toolchain: OnceLock::new(),
            clippy_toml: OnceLock::new(),
            member_clippy_tomls: OnceLock::new(),
            deny_toml: OnceLock::new(),
            pyproject: OnceLock::new(),
            python_version: OnceLock::new(),
            git_archival: OnceLock::new(),
            eslint_config: OnceLock::new(),
            package_json: OnceLock::new(),
            pnpm_workspace: OnceLock::new(),
            prettier: OnceLock::new(),
            tsconfig: OnceLock::new(),
            typescript_referenced_configs: OnceLock::new(),
        }
    }

    /// The component this context is for.
    pub(crate) const fn component(&self) -> &Component {
        &self.component
    }

    /// The component's directory.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The license texts in the project's `LICENSES/` and their copies at the project root, which the component's
    /// manifest names its license from. See [`ProjectContext::license_files`].
    pub(crate) fn license_files(&self) -> &LicenseFiles {
        &self.license_files
    }

    /// Parsed `Cargo.toml` (cached).
    pub(crate) fn cargo(&self) -> &ConfigStatus<CargoToml> {
        self.cargo
            .get_or_init(|| parse_toml_file::<CargoToml>(&self.path.join("Cargo.toml")))
    }

    /// The members of the Cargo workspace whose root manifest is the component's `Cargo.toml` (cached), other than the
    /// package in that manifest. There are none if it has no `[workspace]` table, or doesn't parse.
    pub(crate) fn cargo_members(&self) -> &CargoWorkspaceMembers {
        self.cargo_members.get_or_init(|| {
            self.cargo()
                .as_ref_ok()
                .map(|manifest| find_members(&self.path, manifest))
                .unwrap_or_default()
        })
    }

    /// Parsed `.cargo/config.toml` in the component's own directory (cached). For a component at the root, this is
    /// [`ProjectContext::cargo_config`].
    pub(crate) fn cargo_config(&self) -> &ConfigStatus<CargoConfig> {
        self.cargo_config
            .get_or_init(|| parse_toml_file::<CargoConfig>(&self.path.join(".cargo/config.toml")))
    }

    /// The toolchain file that rustup uses in the component (cached): the nearest `rust-toolchain.toml` or legacy
    /// `rust-toolchain` in the component's directory or a parent, up to the project root, with its path relative to
    /// the component. A legacy file is reported as `NotFound`, since only `rust-toolchain.toml` is read; if there is
    /// no file at all, the path is the bare `rust-toolchain.toml`.
    pub(crate) fn rust_toolchain(&self) -> &(String, ConfigStatus<RustToolchain>) {
        self.rust_toolchain.get_or_init(|| {
            for (depth, dir) in self.component.path.ancestors().enumerate() {
                let dir = self.root.join(dir);
                let up = "../".repeat(depth);

                // rustup prefers the legacy file when a directory has both.
                if dir.join(LEGACY_RUST_TOOLCHAIN).is_file() {
                    return (
                        format!("{up}{LEGACY_RUST_TOOLCHAIN}"),
                        ConfigStatus::NotFound,
                    );
                }
                let path = dir.join(RUST_TOOLCHAIN);
                if path.is_file() {
                    return (
                        format!("{up}{RUST_TOOLCHAIN}"),
                        parse_toml_file::<RustToolchain>(&path),
                    );
                }
            }

            (RUST_TOOLCHAIN.to_owned(), ConfigStatus::NotFound)
        })
    }

    /// The Clippy configuration that applies to the component's package (cached): the nearest `.clippy.toml` or
    /// `clippy.toml` in the component's directory or a parent, up to the project root, with its path relative to the
    /// component. If there is neither, the path is the bare `clippy.toml`. A `.clippy.toml` is reported as `NotFound`,
    /// since only `clippy.toml` is read.
    pub(crate) fn clippy_toml(&self) -> &(String, ConfigStatus<ClippyToml>) {
        self.clippy_toml.get_or_init(|| {
            let Some(found) = nearest_clippy_toml(&self.root, &self.component.path) else {
                return (CLIPPY_TOML.to_owned(), ConfigStatus::NotFound);
            };
            let depth = self
                .component
                .path
                .ancestors()
                .position(|dir| found.parent() == Some(dir))
                .unwrap_or_default();
            let name = found.file_name().unwrap_or_default().to_string_lossy();
            (
                format!("{}{name}", "../".repeat(depth)),
                read_clippy_toml(&self.root, &found),
            )
        })
    }

    /// The Clippy configurations that apply to members of the component's Cargo workspace instead of the component's
    /// own (cached), with their paths relative to the component. Clippy looks for its configuration from each
    /// package's own directory, so a file in a member's directory, or in a directory between it and the component,
    /// applies to that member. Each is listed once, however many members it applies to.
    pub(crate) fn member_clippy_tomls(&self) -> &[(String, ConfigStatus<ClippyToml>)] {
        self.member_clippy_tomls.get_or_init(|| {
            let own = nearest_clippy_toml(&self.root, &self.component.path);
            let found: BTreeSet<PathBuf> = self
                .cargo_members()
                .members
                .keys()
                .filter_map(|member| {
                    nearest_clippy_toml(&self.root, &self.component.path.join(member))
                })
                .filter(|found| Some(found) != own.as_ref())
                .collect();
            found
                .into_iter()
                .filter_map(|found| {
                    let label = found.strip_prefix(&self.component.path).ok()?;
                    let label: Vec<_> = label.iter().map(|part| part.to_string_lossy()).collect();
                    Some((label.join("/"), read_clippy_toml(&self.root, &found)))
                })
                .collect()
        })
    }

    /// Parsed `deny.toml` in the component's own directory (cached).
    pub(crate) fn deny_toml(&self) -> &ConfigStatus<DenyToml> {
        self.deny_toml
            .get_or_init(|| parse_toml_file::<DenyToml>(&self.path.join("deny.toml")))
    }

    /// Parsed `pyproject.toml` (cached).
    pub(crate) fn pyproject(&self) -> &ConfigStatus<PyprojectToml> {
        self.pyproject
            .get_or_init(|| parse_toml_file::<PyprojectToml>(&self.path.join("pyproject.toml")))
    }

    /// Parsed `.python-version` in the component's own directory (cached). uv reads it only at the root of the
    /// project's workspace, which a Python component always is, since nested Python markers are folded into it.
    pub(crate) fn python_version(&self) -> &ConfigStatus<Version> {
        self.python_version
            .get_or_init(|| match read_text_file(&self.path.join(PYTHON_VERSION)) {
                ConfigStatus::Ok(raw) => match parse_python_version(&raw) {
                    Ok(version) => ConfigStatus::Ok(version),
                    Err(error) => ConfigStatus::ParseError { error },
                },
                ConfigStatus::ParseError { error } => ConfigStatus::ParseError { error },
                ConfigStatus::NotFound => ConfigStatus::NotFound,
            })
    }

    /// The text of `.git_archival.txt` in the component's own directory (cached), beside `pyproject.toml`, where
    /// hatch-vcs reads it.
    pub(crate) fn git_archival(&self) -> &ConfigStatus<String> {
        self.git_archival
            .get_or_init(|| read_text_file(&self.path.join(GIT_ARCHIVAL)))
    }

    /// The text of `eslint.config.ts` (cached), which is code rather than data, so checks match its text.
    pub(crate) fn eslint_config(&self) -> &ConfigStatus<String> {
        self.eslint_config
            .get_or_init(|| read_text_file(&self.path.join("eslint.config.ts")))
    }

    /// Parsed `package.json` (cached).
    pub(crate) fn package_json(&self) -> &ConfigStatus<PackageJson> {
        self.package_json
            .get_or_init(|| parse_json5_file::<PackageJson>(&self.path.join("package.json")))
    }

    /// Parsed `pnpm-workspace.yaml` (cached).
    pub(crate) fn pnpm_workspace(&self) -> &ConfigStatus<PnpmWorkspace> {
        self.pnpm_workspace.get_or_init(|| {
            parse_yaml_file::<PnpmWorkspace>(&self.path.join("pnpm-workspace.yaml"))
        })
    }

    /// Parsed `.prettierrc` (cached).
    pub(crate) fn prettier(&self) -> &ConfigStatus<PrettierConfig> {
        self.prettier
            .get_or_init(|| parse_yaml_file::<PrettierConfig>(&self.path.join(".prettierrc")))
    }

    /// Parsed `tsconfig.json` (cached).
    pub(crate) fn tsconfig(&self) -> &ConfigStatus<TsConfig> {
        self.tsconfig
            .get_or_init(|| parse_json5_file::<TsConfig>(&self.path.join("tsconfig.json")))
    }

    /// Parsed referenced tsconfig files for solution-style configurations (cached).
    ///
    /// If `tsconfig.json` contains a `references` array, each referenced path is resolved relative to the component
    /// and parsed. Returns a vec of `(display_name, ConfigStatus<TsConfig>)` pairs.
    ///
    /// Returns an empty vec if the root tsconfig is not solution-style, could not be parsed, or has no references.
    pub(crate) fn typescript_referenced_configs(&self) -> &[(String, ConfigStatus<TsConfig>)] {
        self.typescript_referenced_configs.get_or_init(|| {
            let Some(root) = self.tsconfig().as_ref_ok() else {
                return Vec::new();
            };

            if !root.is_solution_style() {
                return Vec::new();
            }

            root.reference_paths()
                .into_iter()
                .map(|ref_path| {
                    // Resolve the reference path relative to the component.
                    // References can point to either a directory (containing tsconfig.json) or a file directly.
                    let resolved = self.path.join(ref_path);
                    let config_path = if resolved.is_dir() {
                        resolved.join("tsconfig.json")
                    } else if resolved.extension().is_some() {
                        resolved
                    } else {
                        // Bare name without extension - TypeScript treats as directory
                        resolved.join("tsconfig.json")
                    };

                    let display = config_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();

                    let status = parse_json5_file::<TsConfig>(&config_path);
                    (display, status)
                })
                .collect()
        })
    }
}

//
// Parsing Helpers
//

/// Read a text file, returning `NotFound` if it doesn't exist.
fn read_text_file(path: &Path) -> ConfigStatus<String> {
    if !path.exists() {
        return ConfigStatus::NotFound;
    }

    match fs::read_to_string(path) {
        Ok(content) => ConfigStatus::Ok(content),
        Err(e) => ConfigStatus::ParseError {
            error: format!("Could not read file: {e}"),
        },
    }
}

/// The nearest Clippy configuration file to `dir`, both relative to the project root `root`: the first `.clippy.toml`
/// or `clippy.toml` in `dir` or a parent, up to the root, as Clippy finds it from a package's directory. Clippy prefers
/// `.clippy.toml` when a directory has both.
fn nearest_clippy_toml(root: &Path, dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|dir| {
        [HIDDEN_CLIPPY_TOML, CLIPPY_TOML]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|path| root.join(path).is_file())
    })
}

/// Parse the Clippy configuration file `found`, relative to the project root `root`, unless it is a `.clippy.toml`,
/// which is reported as `NotFound`, since only `clippy.toml` is read.
fn read_clippy_toml(root: &Path, found: &Path) -> ConfigStatus<ClippyToml> {
    if found
        .file_name()
        .is_some_and(|name| name == HIDDEN_CLIPPY_TOML)
    {
        ConfigStatus::NotFound
    } else {
        parse_toml_file(&root.join(found))
    }
}

/// Parse a TOML file into the given type.
pub(crate) fn parse_toml_file<T: serde::de::DeserializeOwned>(path: &Path) -> ConfigStatus<T> {
    if !path.exists() {
        return ConfigStatus::NotFound;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return ConfigStatus::ParseError {
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match toml::from_str::<T>(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            error: format!("Invalid TOML: {e}"),
        },
    }
}

/// Parse a JSON/JSON5 file into the given type.
fn parse_json5_file<T: serde::de::DeserializeOwned>(path: &Path) -> ConfigStatus<T> {
    if !path.exists() {
        return ConfigStatus::NotFound;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return ConfigStatus::ParseError {
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match json5::from_str(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            error: format!("Invalid JSON: {e}"),
        },
    }
}

/// Parse a YAML file into the given type.
fn parse_yaml_file<T: serde::de::DeserializeOwned>(path: &Path) -> ConfigStatus<T> {
    if !path.exists() {
        return ConfigStatus::NotFound;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return ConfigStatus::ParseError {
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match serde_saphyr::from_str(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            error: format!("Invalid YAML: {e}"),
        },
    }
}

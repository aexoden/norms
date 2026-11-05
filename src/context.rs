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
//! fn check_devbox_packages(ctx: &ProjectContext, report: &mut Report) {
//!     // This call is free if another check already triggered the parse.
//!     let Some(config) = report.require_parsed("devbox.json", ctx.devbox()) else { return };
//!     report.should("devbox.json: has packages", !config.packages.is_empty());
//! }
//! ```
//!
//! NOTE: This specific Devbox functionality does not yet exist -- this is more of a planning note.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::{
    CargoConfig, CargoToml, DevboxConfig, NormsConfig, PackageJson, PnpmWorkspace, PrecommitConfig,
    PyprojectToml, RenovateConfig, TsConfig, Workflow,
};
use crate::detection::Detection;
use crate::models::{Component, Language};

/// The file name of the CI workflow in `.github/workflows/`.
pub(crate) const CI_WORKFLOW: &str = "ci.yaml";

/// The result of attempting to read and parse a config file.
///
/// This three-state enum lets checks distinguish between "file missing",
/// "file present but malformed", and "file present and valid". The raw content
/// is preserved in the `ParseError` variant so that checks can still do
/// text-level matching even if structured parsing failed.
#[derive(Debug)]
pub(crate) enum ConfigStatus<T> {
    /// The file does not exist.
    NotFound,
    /// The file exists but could not be parsed.
    ParseError {
        /// The raw file content (useful for fallback text matching).
        raw: String,
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

    /// Get the raw file content if the file exists (regardless of parse success).
    #[expect(dead_code, reason = "placeholder for future config getters")]
    pub(crate) fn raw_content(&self) -> Option<&str> {
        match self {
            Self::ParseError { raw, .. } => Some(raw),
            Self::NotFound | Self::Ok(_) => None,
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
    languages: OnceLock<BTreeSet<Language>>,
    cargo_config: OnceLock<ConfigStatus<CargoConfig>>,
    devbox: OnceLock<ConfigStatus<DevboxConfig>>,
    norms: OnceLock<ConfigStatus<NormsConfig>>,
    renovate: OnceLock<ConfigStatus<RenovateConfig>>,
    precommit: OnceLock<ConfigStatus<PrecommitConfig>>,
    editorconfig: OnceLock<ConfigStatus<String>>,
    gitattributes: OnceLock<ConfigStatus<String>>,
    workflows: OnceLock<BTreeMap<String, ConfigStatus<Workflow>>>,
}

impl ProjectContext {
    /// Create a new context for the project at the given path.
    pub(crate) const fn new(path: PathBuf) -> Self {
        Self {
            path,
            detection: OnceLock::new(),
            languages: OnceLock::new(),
            cargo_config: OnceLock::new(),
            devbox: OnceLock::new(),
            norms: OnceLock::new(),
            renovate: OnceLock::new(),
            precommit: OnceLock::new(),
            editorconfig: OnceLock::new(),
            gitattributes: OnceLock::new(),
            workflows: OnceLock::new(),
        }
    }

    /// The root path of the project.
    pub(crate) fn path(&self) -> &Path {
        &self.path
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
}

/// Lazy-loading context for one component of a project.
///
/// Like [`ProjectContext`], each config file is parsed at most once. Paths are relative to the component's directory,
/// which may be the project root or a subdirectory. A component has a single language, so only that language's
/// getters are ever called.
pub(crate) struct ComponentContext {
    path: PathBuf,
    cargo: OnceLock<ConfigStatus<CargoToml>>,
    cargo_config: OnceLock<ConfigStatus<CargoConfig>>,
    pyproject: OnceLock<ConfigStatus<PyprojectToml>>,
    package_json: OnceLock<ConfigStatus<PackageJson>>,
    pnpm_workspace: OnceLock<ConfigStatus<PnpmWorkspace>>,
    tsconfig: OnceLock<ConfigStatus<TsConfig>>,
    typescript_referenced_configs: OnceLock<Vec<(String, ConfigStatus<TsConfig>)>>,
}

impl ComponentContext {
    /// Create a new context for the given component of the project at `root`.
    pub(crate) fn new(root: &Path, component: &Component) -> Self {
        Self {
            path: root.join(&component.path),
            cargo: OnceLock::new(),
            cargo_config: OnceLock::new(),
            pyproject: OnceLock::new(),
            package_json: OnceLock::new(),
            pnpm_workspace: OnceLock::new(),
            tsconfig: OnceLock::new(),
            typescript_referenced_configs: OnceLock::new(),
        }
    }

    /// The component's directory.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Parsed `Cargo.toml` (cached).
    pub(crate) fn cargo(&self) -> &ConfigStatus<CargoToml> {
        self.cargo
            .get_or_init(|| parse_toml_file::<CargoToml>(&self.path.join("Cargo.toml")))
    }

    /// Parsed `.cargo/config.toml` in the component's own directory (cached). For a component at the root, this is
    /// [`ProjectContext::cargo_config`].
    pub(crate) fn cargo_config(&self) -> &ConfigStatus<CargoConfig> {
        self.cargo_config
            .get_or_init(|| parse_toml_file::<CargoConfig>(&self.path.join(".cargo/config.toml")))
    }

    /// Parsed `pyproject.toml` (cached).
    pub(crate) fn pyproject(&self) -> &ConfigStatus<PyprojectToml> {
        self.pyproject
            .get_or_init(|| parse_toml_file::<PyprojectToml>(&self.path.join("pyproject.toml")))
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
            raw: String::new(),
            error: format!("Could not read file: {e}"),
        },
    }
}

// Parse a TOML file into the given type.
fn parse_toml_file<T: serde::de::DeserializeOwned>(path: &Path) -> ConfigStatus<T> {
    if !path.exists() {
        return ConfigStatus::NotFound;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return ConfigStatus::ParseError {
                raw: String::new(),
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match toml::from_str::<T>(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            raw,
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
                raw: String::new(),
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match json5::from_str(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            raw,
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
                raw: String::new(),
                error: format!("Could not read file: {e}"),
            };
        }
    };

    match serde_saphyr::from_str(&raw) {
        Ok(value) => ConfigStatus::Ok(value),
        Err(e) => ConfigStatus::ParseError {
            raw,
            error: format!("Invalid YAML: {e}"),
        },
    }
}

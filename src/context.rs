// SPDX-License-Identifier: Apache-2.0 OR MIT
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

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::{DevboxConfig, RenovateConfig};
use crate::models::Language;

/// The result of attempting to read and parse a config file.
///
/// This three-state enum lets checks distinguish between "file missing",
/// "file present but malformed", and "file present and valid". The raw content
/// is preserved in the `ParseError` variant so that checks can still do
/// text-level matching even if structured parsing failed.
#[derive(Debug)]
pub enum ConfigStatus<T> {
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
    #[expect(dead_code)] // Placeholder for future config getters
    pub fn as_ref_ok(&self) -> Option<&T> {
        match self {
            Self::Ok(v) => Some(v),
            _ => None,
        }
    }

    /// Get the raw file content if the file exists (regardless of parse success).
    #[expect(dead_code)] // Placeholder for future config getters
    pub fn raw_content(&self) -> Option<&str> {
        match self {
            Self::ParseError { raw, .. } => Some(raw),
            _ => None,
        }
    }
}

/// Lazy-loading project context.
///
/// Each config file is parsed at most once using [`OnceLock`]. The first call to
/// a getter triggers the parse; subsequent calls return the cached result.
pub struct ProjectContext {
    path: PathBuf,
    languages: OnceLock<HashSet<Language>>,
    devbox: OnceLock<ConfigStatus<DevboxConfig>>,
    renovate: OnceLock<ConfigStatus<RenovateConfig>>,
    editorconfig: OnceLock<ConfigStatus<String>>,
    gitattributes: OnceLock<ConfigStatus<String>>,
}

impl ProjectContext {
    /// Create a new context for the project at the given path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            languages: OnceLock::new(),
            devbox: OnceLock::new(),
            renovate: OnceLock::new(),
            editorconfig: OnceLock::new(),
            gitattributes: OnceLock::new(),
        }
    }

    /// The root path of the project.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Detected programming languages (cached).
    pub fn languages(&self) -> &HashSet<Language> {
        self.languages
            .get_or_init(|| crate::detection::detect_languages(&self.path))
    }

    /// Parsed `devbox.json` (cached). Uses JSON5 for flexibility.
    pub fn devbox(&self) -> &ConfigStatus<DevboxConfig> {
        self.devbox
            .get_or_init(|| parse_json5_file::<DevboxConfig>(&self.path.join("devbox.json")))
    }

    /// Parsed `renovate.json` (cached). Uses JSON5 for flexibility.
    pub fn renovate(&self) -> &ConfigStatus<RenovateConfig> {
        self.renovate
            .get_or_init(|| parse_json5_file::<RenovateConfig>(&self.path.join("renovate.json")))
    }

    // Raw `.editorconfig` content (cached).
    pub fn editorconfig(&self) -> &ConfigStatus<String> {
        self.editorconfig
            .get_or_init(|| read_text_file(&self.path.join(".editorconfig")))
    }

    // Raw `.gitattributes` content (cached).
    pub fn gitattributes(&self) -> &ConfigStatus<String> {
        self.gitattributes
            .get_or_init(|| read_text_file(&self.path.join(".gitattributes")))
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

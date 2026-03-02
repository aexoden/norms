// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `Cargo.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level `Cargo.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CargoToml {
    pub package: Option<CargoPackage>,
    pub lints: Option<CargoLints>,
    pub workspace: Option<CargoWorkspace>,
    /// Explicit `[[bin]]` targets.
    #[serde(default)]
    pub bin: Vec<CargoBinTarget>,
}

/// The `[package]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct CargoPackage {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    pub repository: Option<String>,
}

/// The `[lints]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CargoLints {
    pub clippy: Option<HashMap<String, toml::Value>>,
    pub rust: Option<HashMap<String, toml::Value>>,
}

/// The `[workspace]` table (subset).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CargoWorkspace {
    pub lints: Option<CargoLints>,
}

/// A `[[bin]]` target entry.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CargoBinTarget {
    pub name: Option<String>,
}

impl CargoToml {
    /// Whether the project defines any binary targets (explicit `[[bin]]` or inferred from `src/main.rs`).
    ///
    /// Note: the caller must separately check for `src/main.rs` on the filesystem.
    pub fn has_explicit_bin_targets(&self) -> bool {
        !self.bin.is_empty()
    }

    /// Get the effective Clippy lint configuration, preferring package-level over workspace-level.
    pub fn effective_clippy_lints(&self) -> Option<&HashMap<String, toml::Value>> {
        self.lints
            .as_ref()
            .and_then(|l| l.clippy.as_ref())
            .or_else(|| {
                self.workspace
                    .as_ref()
                    .and_then(|w| w.lints.as_ref())
                    .and_then(|l| l.clippy.as_ref())
            })
    }

    /// Check whether a specific Clippy lint group or lint is configured.
    pub fn has_clippy_lint(&self, name: &str) -> bool {
        self.effective_clippy_lints()
            .is_some_and(|lints| lints.contains_key(name))
    }
}

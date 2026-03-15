// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `tsconfig.json`.
//!
//! Since `tsconfig.json` supports JSON with Comments (JSONC), parsing uses the `json5` crate which handles both
//! single-line comments and trailing commas.
//!
//! Only the subset of fields inspected by checks is modeled.

use serde::Deserialize;

/// Top-level `tsconfig.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TsConfig {
    pub compiler_options: Option<TsCompilerOptions>,

    /// Files to extend from
    pub extends: Option<serde_json::Value>,

    /// Project references for solution-style tsconfig.
    ///
    /// When present, the root tsconfig delegates compilation to referenced sub-projects (e.g. `tsconfig.app.json`,
    /// `tsconfig.node.json`). The root typically has no `compilerOptions` of its own.
    #[serde(default)]
    pub references: Vec<TsReference>,
}

/// A single entry in the `references` array.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct TsReference {
    /// Path to the referenced tsconfig file (relative to the root tsconfig).
    pub path: String,
}

impl TsConfig {
    /// Whether htis is a solution-style tsconfig (uses project references).
    ///
    /// Solution-style configs have a non-empty `references` array and typically no `compilerOptions`, delegating all
    /// compilation to the referenced sub-projects.
    pub fn is_solution_style(&self) -> bool {
        !self.references.is_empty()
    }

    /// Return the reference paths as a slice of strings.
    pub fn reference_paths(&self) -> Vec<&str> {
        self.references.iter().map(|r| r.path.as_str()).collect()
    }
}

/// The `compilerOptions` section of `tsconfig.json`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TsCompilerOptions {
    pub strict: Option<bool>,
    pub no_unchecked_indexed_access: Option<bool>,
    pub module: Option<String>,
    pub target: Option<String>,
    pub module_resolution: Option<String>,
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
    pub declaration: Option<bool>,
    pub source_map: Option<bool>,
    pub es_module_interop: Option<bool>,
    pub skip_lib_check: Option<bool>,
    pub force_consistent_casing_in_file_names: Option<bool>,
    pub resolve_json_module: Option<bool>,
    pub isolated_modules: Option<bool>,
    pub verbatim_module_syntax: Option<bool>,
}

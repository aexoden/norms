// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `package.json`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level `package.json` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PackageJson {
    pub name: Option<String>,
    pub description: Option<String>,

    /// Module type (`"module"` for ESM, `"commonjs"` or absent for CJS).
    #[serde(rename = "type")]
    pub module_type: Option<String>,

    /// Corepack-style package manager declaration (e.g. `"pnpm@9.15.0"`)
    pub package_manager: Option<String>,

    /// Engine constraints (e.g. `{ "node": ">=20" }`).
    pub engines: Option<HashMap<String, String>>,

    /// npm scripts.
    pub scripts: Option<HashMap<String, String>>,

    /// Production dependencies.
    pub dependencies: Option<HashMap<String, String>>,

    // Development dependencies.
    pub dev_dependencies: Option<HashMap<String, String>>,
}

impl PackageJson {
    /// Whether `type` is set to `"module"` (ESM).
    pub fn is_esm(&self) -> bool {
        self.module_type.as_deref() == Some("module")
    }

    /// Whether `packageManager` starts with `"pnpm"`.
    pub fn uses_pnpm(&self) -> bool {
        self.package_manager
            .as_ref()
            .is_some_and(|pm| pm.starts_with("pnpm"))
    }

    /// Get the `packageManager` value for display in messages.
    pub fn package_manager_display(&self) -> Option<&str> {
        self.package_manager.as_deref()
    }

    /// Whether a given package name appears in any dependency list.
    pub fn has_dep(&self, name: &str) -> bool {
        self.dependencies
            .as_ref()
            .is_some_and(|d| d.contains_key(name))
            || self
                .dev_dependencies
                .as_ref()
                .is_some_and(|d| d.contains_key(name))
    }

    /// Whether any dependency name starts with the given prefix.
    pub fn has_dep_starting_with(&self, prefix: &str) -> bool {
        let check = |deps: &HashMap<String, String>| deps.keys().any(|k| k.starts_with(prefix));
        self.dependencies.as_ref().is_some_and(check)
            || self.dev_dependencies.as_ref().is_some_and(check)
    }

    /// Whether a script with the given name exists.
    pub fn has_script(&self, name: &str) -> bool {
        self.scripts.as_ref().is_some_and(|s| s.contains_key(name))
    }

    /// Whether any script name matches one of the given alternatives.
    pub fn has_any_script(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.has_script(n))
    }
}

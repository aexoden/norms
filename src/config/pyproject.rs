// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `pyproject.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level `pyproject.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct PyprojectToml {
    pub project: Option<ProjectSection>,
    pub tool: Option<ToolSection>,
    pub dependency_groups: Option<HashMap<String, Vec<toml::Value>>>,
}

/// The `[project]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ProjectSection {
    pub name: Option<String>,
    pub description: Option<String>,
    pub requires_python: Option<String>,
    pub license: Option<toml::Value>,
}

/// The `[tool]` table (we only model the tools we inspect)
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ToolSection {
    pub ruff: Option<RuffConfig>,
    pub mypy: Option<MypyConfig>,
}

//
// Ruff
//

/// `[tool.ruff]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuffConfig {
    pub line_length: Option<u32>,
    pub format: Option<RuffFormatConfig>,
    pub lint: Option<RuffLintConfig>,
}

/// `[tool.ruff.format]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuffFormatConfig {
    pub line_ending: Option<String>,
}

/// `[tool.ruff.lint]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuffLintConfig {
    pub preview: Option<bool>,
    pub select: Option<Vec<String>>,
    pub ignore: Option<Vec<String>>,
    pub pydocstyle: Option<RuffLintPydocstyleConfig>,
    pub isort: Option<RuffLintIsortConfig>,
    pub flake8_copyright: Option<RuffLintCopyrightConfig>,
}

impl RuffLintConfig {
    /// Whether the `select` list includes `"ALL"`.
    pub fn selects_all(&self) -> bool {
        self.select
            .as_ref()
            .is_some_and(|s| s.iter().any(|r| r == "ALL"))
    }

    /// Whether any rules are selected at all.
    pub fn has_rules(&self) -> bool {
        self.select.as_ref().is_some_and(|s| !s.is_empty())
    }

    /// Whether an ignore list is present and non-empty.
    pub fn has_ignores(&self) -> bool {
        self.ignore.as_ref().is_some_and(|i| !i.is_empty())
    }
}

/// `[tool.ruff.lint.pydocstyle]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RuffLintPydocstyleConfig {
    pub convention: Option<String>,
}

/// `[tool.ruff.lint.isort]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuffLintIsortConfig {
    pub lines_between_types: Option<u32>,
}

/// `[tool.ruff.lint.flake8-copyright]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuffLintCopyrightConfig {
    pub notice_rgx: Option<String>,
}

//
// Mypy
//

/// `[tool.mypy]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct MypyConfig {
    pub strict: Option<bool>,
}

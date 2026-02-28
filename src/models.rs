// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Core data models for the norms tool.

use std::fmt;

use serde::Serialize;

/// Supported programming languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Cpp,
    Python,
    Rust,
    TypeScript,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpp => write!(f, "C++"),
            Self::Python => write!(f, "Python"),
            Self::Rust => write!(f, "Rust"),
            Self::TypeScript => write!(f, "TypeScript"),
        }
    }
}

/// The severity/outcome of a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Warn,
    Skip,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail => write!(f, "FAIL"),
            Self::Warn => write!(f, "WARN"),
            Self::Skip => write!(f, "SKIP"),
        }
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Core data models for the norms tool.

use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

/// Supported programming languages.
///
/// The order of the variants is the order in which languages are checked and listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Language {
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

/// A component of a project: the directory holding one language's marker file.
///
/// Components sort by language, then by path, which is the order in which they are checked and listed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Component {
    pub language: Language,
    /// The component's directory, relative to the project root; empty for the root itself.
    pub path: PathBuf,
}

impl Component {
    /// The component's directory as `/`-separated text, or `.` for the project root.
    pub(crate) fn display_path(&self) -> String {
        if self.path.as_os_str().is_empty() {
            return ".".into();
        }

        self.path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// The severity/outcome of a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_is_slash_separated_with_a_dot_for_the_root() {
        let component = |path: &str| Component {
            language: Language::TypeScript,
            path: PathBuf::from(path),
        };

        assert_eq!(component("").display_path(), ".");
        assert_eq!(component("frontend").display_path(), "frontend");
        assert_eq!(component("apps/web").display_path(), "apps/web");
    }

    #[test]
    fn components_sort_by_language_then_path() {
        let mut components = [
            Component {
                language: Language::TypeScript,
                path: PathBuf::from("b"),
            },
            Component {
                language: Language::TypeScript,
                path: PathBuf::from("a"),
            },
            Component {
                language: Language::Rust,
                path: PathBuf::from("z"),
            },
        ];
        components.sort();

        let order: Vec<_> = components.iter().map(Component::display_path).collect();
        assert_eq!(order, ["z", "a", "b"]);
    }
}

// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Language detection utilities.

use std::collections::HashSet;
use std::path::Path;

use crate::models::Language;

/// Detect programming languages used in the project at the given path.
///
/// Detection is based on the presence of canonical build/config files:
/// - `CMakeLists.txt` for C++
/// - `pyproject.toml` for Python
/// - `Cargo.toml` for Rust
/// - `package.json` for TypeScript
pub fn detect_languages(path: &Path) -> HashSet<Language> {
    let mut languages = HashSet::new();

    let markers: &[(Language, &str)] = &[
        (Language::Cpp, "CMakeLists.txt"),
        (Language::Python, "pyproject.toml"),
        (Language::Rust, "Cargo.toml"),
        (Language::TypeScript, "package.json"),
    ];

    // TODO: This doesn't detect subdirectories.
    for &(language, marker) in markers {
        if path.join(marker).exists() {
            languages.insert(language);
        }
    }

    languages
}

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

    for &(language, marker) in markers {
        if path.join(marker).exists() {
            languages.insert(language);
        }
    }

    // TypeScript projects may live in a frontend/ subdirectory
    if !languages.contains(&Language::TypeScript) {
        let frontend = path.join("frontend");
        if frontend.is_dir() && frontend.join("package.json").exists() {
            languages.insert(Language::TypeScript);
        }
    }

    languages
}

/// Resolve the TypeScript project root directory.
///
/// TypeScript projects may live at the repository root or in a `frontend/` subdirectory. This function returns the path
/// containing `package.json`, preferring the repository root if it exists there.
pub fn resolve_typescript_root(path: &Path) -> std::path::PathBuf {
    if path.join("package.json").exists() {
        return path.to_path_buf();
    }

    let frontend = path.join("frontend");
    if frontend.is_dir() && frontend.join("package.json").exists() {
        return frontend;
    }

    // Fall back to repository root, though without a package.json TypeScript checks will be skipped
    path.to_path_buf()
}

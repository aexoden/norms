// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Component detection utilities.

use std::path::{Path, PathBuf};

use crate::models::{Component, Language};

/// The marker file of each language, whose directory is a component of that language.
const MARKERS: &[(Language, &str)] = &[
    (Language::Cpp, "CMakeLists.txt"),
    (Language::Python, "pyproject.toml"),
    (Language::Rust, "Cargo.toml"),
    (Language::TypeScript, "package.json"),
];

/// Detect the components of the project at the given path, in a stable order.
///
/// Detection is based on the presence of canonical build/config files:
/// - `CMakeLists.txt` for C++
/// - `pyproject.toml` for Python
/// - `Cargo.toml` for Rust
/// - `package.json` for TypeScript
///
/// Each marker at the root makes the root a component. TypeScript may instead live in a `frontend/` subdirectory,
/// which is used only if the root has no `package.json`.
pub(crate) fn detect_components(path: &Path) -> Vec<Component> {
    let mut components: Vec<Component> = MARKERS
        .iter()
        .filter(|&&(_, marker)| path.join(marker).exists())
        .map(|&(language, _)| Component {
            language,
            path: PathBuf::new(),
        })
        .collect();

    let frontend = Path::new("frontend");
    if !path.join("package.json").exists() && path.join(frontend).join("package.json").exists() {
        components.push(Component {
            language: Language::TypeScript,
            path: frontend.to_path_buf(),
        });
    }

    components.sort();
    components
}

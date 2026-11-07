// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Component detection utilities.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use crate::models::{Component, Language};

/// The marker file of each language, whose directory is a component of that language.
const MARKERS: &[(Language, &str)] = &[
    (Language::Cpp, "CMakeLists.txt"),
    (Language::Python, "pyproject.toml"),
    (Language::Rust, "Cargo.toml"),
    (Language::TypeScript, "package.json"),
];

/// Directories of build output and installed dependencies, which are never searched even if `.gitignore` doesn't
/// exclude them.
const SKIPPED_DIRS: &[&str] = &[".venv", "node_modules", "target"];

/// The configuration files of markdownlint-cli2, and of markdownlint, which it reads in any directory of the files it
/// lints.
const MARKDOWNLINT_CONFIGS: &[&str] = &[
    ".markdownlint-cli2.jsonc",
    ".markdownlint-cli2.yaml",
    ".markdownlint-cli2.cjs",
    ".markdownlint-cli2.mjs",
    ".markdownlint.jsonc",
    ".markdownlint.json",
    ".markdownlint.yaml",
    ".markdownlint.yml",
    ".markdownlint.cjs",
    ".markdownlint.mjs",
];

/// Find the project's markdownlint configuration files, by their paths relative to the root, in a stable order.
///
/// markdownlint-cli2 lints hidden directories too, and doesn't skip build output or installed dependencies, so the
/// search covers every directory the project commits: only what its own `.gitignore` files exclude, and `.git`, are
/// skipped. Like component detection, it doesn't descend into symlinked directories. A directory that can't be read is
/// left out.
pub(crate) fn find_markdownlint_configs(root: &Path) -> Vec<PathBuf> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| !(entry.depth() > 0 && entry.file_name() == ".git"))
        .build();

    let mut configs: Vec<PathBuf> = walker
        .filter_map(Result::ok)
        .filter(|entry| {
            MARKDOWNLINT_CONFIGS
                .iter()
                .any(|&name| entry.file_name() == name)
                && entry
                    .file_type()
                    .is_some_and(|file_type| !file_type.is_dir())
        })
        .filter_map(|entry| Some(entry.path().strip_prefix(root).ok()?.to_path_buf()))
        .collect();
    configs.sort();
    configs
}

/// The result of searching a project for its components.
#[derive(Debug, Default)]
pub(crate) struct Detection {
    /// The components found, in a stable order.
    pub components: Vec<Component>,
    /// The errors that kept part of the project from being searched, such as an unreadable directory.
    pub errors: Vec<String>,
}

/// Detect the components of the project at the given path, in a stable order.
///
/// Detection is based on the presence of canonical build/config files, at the root or in any subdirectory:
/// - `CMakeLists.txt` for C++
/// - `pyproject.toml` for Python
/// - `Cargo.toml` for Rust
/// - `package.json` for TypeScript
///
/// The search covers only what the project would commit: it skips hidden files and directories, whatever the
/// project's own `.gitignore` files exclude, the directories in [`SKIPPED_DIRS`], and the `excluded` paths (relative
/// to the root). It doesn't descend into symlinked directories, and counts a symlinked marker only if it resolves to a
/// file inside the root, so the search never leaves the project.
///
/// A marker inside a component of the same language belongs to that component, as a Cargo workspace member or pnpm
/// workspace package does, so only the outermost directory with a given marker is a component. Components of
/// different languages may nest.
///
/// A directory that can't be read is left out, and the error recorded in the [`Detection`].
pub(crate) fn detect_components(root: &Path, excluded: &[PathBuf]) -> Detection {
    let canonical_root = root.canonicalize().ok();
    let filter_root = root.to_path_buf();
    let excluded = excluded.to_vec();

    let walker = WalkBuilder::new(root)
        // Only the project's own .gitignore files apply, so that detection doesn't depend on where the project lives
        // or on the user's git configuration; ripgrep's .ignore files don't apply at all.
        .ignore(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| !is_skipped(entry, &filter_root, &excluded))
        .build();

    let mut errors = Vec::new();
    let mut markers: Vec<Component> = walker
        .filter_map(|entry| entry.map_err(|error| errors.push(error.to_string())).ok())
        .filter_map(|entry| {
            let language = marker_language(entry.file_name())?;
            let file_type = entry.file_type()?;

            let is_marker = if file_type.is_symlink() {
                canonical_root
                    .as_deref()
                    .is_some_and(|canonical| resolves_to_file_inside(entry.path(), canonical))
            } else {
                !file_type.is_dir()
            };
            if !is_marker {
                return None;
            }

            let path = entry
                .path()
                .parent()?
                .strip_prefix(root)
                .ok()?
                .to_path_buf();
            Some(Component { language, path })
        })
        .collect();

    // A directory sorts before everything inside it, so each component is kept before any marker it encloses.
    markers.sort();

    let mut components: Vec<Component> = Vec::new();
    for marker in markers {
        let enclosed = components.iter().any(|component| {
            component.language == marker.language && marker.path.starts_with(&component.path)
        });

        if !enclosed {
            components.push(marker);
        }
    }

    Detection { components, errors }
}

/// The language whose marker file has the given name, if any.
fn marker_language(file_name: &OsStr) -> Option<Language> {
    MARKERS
        .iter()
        .find(|&&(_, marker)| file_name == marker)
        .map(|&(language, _)| language)
}

/// Whether the search skips the entry, and everything inside it.
fn is_skipped(entry: &DirEntry, root: &Path, excluded: &[PathBuf]) -> bool {
    let is_dir = entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir());
    if is_dir && SKIPPED_DIRS.iter().any(|&dir| entry.file_name() == dir) {
        return true;
    }

    entry
        .path()
        .strip_prefix(root)
        .is_ok_and(|path| excluded.iter().any(|excluded| path == excluded))
}

/// Whether the symlink at `path` resolves to a file inside `root`, which must be canonical.
fn resolves_to_file_inside(path: &Path, root: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|target| target.starts_with(root) && target.is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// A scratch project holding the given files, each empty, at paths relative to its root.
    fn project(files: &[&str]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();

        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }

        dir
    }

    /// The detected components, as `language:path` pairs.
    fn detect(root: &Path, excluded: &[&str]) -> Vec<String> {
        let excluded: Vec<PathBuf> = excluded.iter().map(PathBuf::from).collect();

        detect_components(root, &excluded)
            .components
            .iter()
            .map(|component| format!("{}:{}", component.language, component.display_path()))
            .collect()
    }

    #[test]
    fn finds_markers_at_the_root_and_in_subdirectories() {
        let dir = project(&[
            "Cargo.toml",
            "pyproject.toml",
            "frontend/package.json",
            "native/CMakeLists.txt",
        ]);

        assert_eq!(
            detect(dir.path(), &[]),
            ["C++:native", "Python:.", "Rust:.", "TypeScript:frontend"]
        );
    }

    #[test]
    fn finds_several_components_of_one_language() {
        let dir = project(&["apps/web/package.json", "apps/admin/package.json"]);

        assert_eq!(
            detect(dir.path(), &[]),
            ["TypeScript:apps/admin", "TypeScript:apps/web"]
        );
    }

    #[test]
    fn folds_nested_markers_of_the_same_language() {
        let dir = project(&[
            "Cargo.toml",
            "crates/core/Cargo.toml",
            "frontend/package.json",
            "frontend/packages/ui/package.json",
            "frontend/tools/Cargo.toml",
            "frontend-old/package.json",
        ]);

        assert_eq!(
            detect(dir.path(), &[]),
            ["Rust:.", "TypeScript:frontend", "TypeScript:frontend-old"]
        );
    }

    #[test]
    fn nests_components_of_different_languages() {
        let dir = project(&["frontend/package.json", "frontend/wasm/Cargo.toml"]);

        assert_eq!(
            detect(dir.path(), &[]),
            ["Rust:frontend/wasm", "TypeScript:frontend"]
        );
    }

    #[test]
    fn finds_markdownlint_configs_in_every_committed_directory() {
        let dir = project(&[
            ".markdownlint-cli2.jsonc",
            ".markdownlint.yaml",
            "docs/.markdownlint-cli2.yaml",
            "docs/api/.markdownlint.jsonc",
            "docs/markdownlint.json",
            ".github/.markdownlint.json",
            "node_modules/pkg/.markdownlint.json",
            "build/.markdownlint.json",
            ".git/.markdownlint.json",
        ]);
        fs::write(dir.path().join(".gitignore"), "/build/\n").unwrap();

        let found: Vec<_> = find_markdownlint_configs(dir.path())
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            found,
            [
                ".github/.markdownlint.json",
                ".markdownlint-cli2.jsonc",
                ".markdownlint.yaml",
                "docs/.markdownlint-cli2.yaml",
                "docs/api/.markdownlint.jsonc",
                "node_modules/pkg/.markdownlint.json"
            ]
        );
    }

    #[test]
    fn skips_build_output_dependencies_and_hidden_directories() {
        let dir = project(&[
            "node_modules/left-pad/package.json",
            "web/node_modules/left-pad/package.json",
            "target/package/demo/Cargo.toml",
            ".venv/lib/demo/pyproject.toml",
            ".hidden/package.json",
        ]);

        assert_eq!(detect(dir.path(), &[]), Vec::<String>::new());
    }

    #[test]
    fn skips_what_gitignore_excludes() {
        let dir = project(&["dist/package.json", "web/build/package.json"]);
        fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
        fs::write(dir.path().join("web/.gitignore"), "build/\n").unwrap();

        assert_eq!(detect(dir.path(), &[]), Vec::<String>::new());
    }

    #[test]
    fn skips_excluded_paths() {
        let dir = project(&[
            "Cargo.toml",
            "tests/fixtures/python/pyproject.toml",
            "tests/fixtures/web/package.json",
            "vendor/package.json",
            "tools/pyproject.toml",
        ]);

        assert_eq!(
            detect(dir.path(), &["tests/fixtures", "vendor"]),
            ["Python:tools", "Rust:."]
        );
    }

    #[test]
    fn ignores_directories_named_like_markers() {
        let dir = project(&["package.json/index.js"]);

        assert_eq!(detect(dir.path(), &[]), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn records_directories_it_cannot_read() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = project(&["Cargo.toml", "locked/package.json"]);
        let locked = dir.path().join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        // Root can read the directory anyway, in which case there is no error to record.
        let readable = fs::read_dir(&locked).is_ok();
        let detection = detect_components(dir.path(), &[]);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        if !readable {
            assert_eq!(detection.components.len(), 1);
            assert!(
                matches!(detection.errors.as_slice(), [error] if error.contains("locked")),
                "{:?}",
                detection.errors
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn searches_a_symlinked_root() {
        use std::os::unix::fs::symlink;

        let dir = project(&["shared/package.json", "Cargo.toml"]);
        fs::create_dir_all(dir.path().join("real/web")).unwrap();
        fs::rename(dir.path().join("shared"), dir.path().join("real/shared")).unwrap();
        fs::rename(
            dir.path().join("Cargo.toml"),
            dir.path().join("real/Cargo.toml"),
        )
        .unwrap();
        symlink(
            "../shared/package.json",
            dir.path().join("real/web/package.json"),
        )
        .unwrap();

        let link = dir.path().join("link");
        symlink(dir.path().join("real"), &link).unwrap();

        assert_eq!(
            detect(&link, &[]),
            ["Rust:.", "TypeScript:shared", "TypeScript:web"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn stays_inside_the_root_through_symlinks() {
        use std::os::unix::fs::symlink;

        let outside = project(&["package.json", "app/Cargo.toml", "pyproject.toml"]);
        let dir = project(&["shared/package.json", "lib/pyproject.toml"]);
        let root = dir.path();

        // Symlinked directories aren't searched, inside the root or out.
        symlink(outside.path().join("app"), root.join("app")).unwrap();
        symlink(root.join("lib"), root.join("lib-link")).unwrap();
        symlink(root, root.join("loop")).unwrap();

        // A symlinked marker counts only if it resolves to a file inside the root.
        fs::create_dir_all(root.join("web")).unwrap();
        symlink(
            root.join("shared/package.json"),
            root.join("web/package.json"),
        )
        .unwrap();
        fs::create_dir_all(root.join("outer")).unwrap();
        symlink(
            outside.path().join("package.json"),
            root.join("outer/package.json"),
        )
        .unwrap();
        symlink(
            outside.path().join("pyproject.toml"),
            root.join("pyproject.toml"),
        )
        .unwrap();
        fs::create_dir_all(root.join("broken")).unwrap();
        symlink(root.join("missing"), root.join("broken/package.json")).unwrap();

        assert_eq!(
            detect(root, &[]),
            ["Python:lib", "TypeScript:shared", "TypeScript:web"]
        );
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The Prettier configuration of a TypeScript component's subdirectories.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::config::PackageJson;
use crate::context::parse_json5_file;

/// The Prettier configuration file the standard names.
pub(crate) const PRETTIER_CONFIG: &str = ".prettierrc";

/// The configuration files Prettier looks for after the `prettier` key in `package.json` and `package.yaml`, in the
/// order Prettier 3.9 does. norms doesn't read `package.yaml`, which pnpm projects rarely have.
pub(crate) const PRETTIER_CONFIGS: [&str; 18] = [
    PRETTIER_CONFIG,
    ".prettierrc.json",
    ".prettierrc.yml",
    ".prettierrc.yaml",
    ".prettierrc.json5",
    ".prettierrc.js",
    "prettier.config.js",
    ".prettierrc.ts",
    "prettier.config.ts",
    ".prettierrc.mjs",
    "prettier.config.mjs",
    ".prettierrc.mts",
    "prettier.config.mts",
    ".prettierrc.cjs",
    "prettier.config.cjs",
    ".prettierrc.cts",
    "prettier.config.cts",
    ".prettierrc.toml",
];

/// The directories Prettier 3.9 skips wherever they are: those of version control systems, and `node_modules`.
const IGNORED_DIRS: [&str; 6] = [".git", ".hg", ".jj", ".sl", ".svn", "node_modules"];

/// The files whose patterns Prettier 3.9 ignores paths by, read only from the directory it runs in.
const IGNORE_FILES: [&str; 2] = [".gitignore", ".prettierignore"];

/// Where Prettier takes its configuration from in one directory below a component's root.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NestedPrettierConfig {
    /// The `prettier` key in the directory's `package.json`.
    PackageJson(PathBuf),
    /// A configuration file, such as `.prettierrc`.
    File(PathBuf),
}

/// The Prettier configurations below a component's root.
#[derive(Debug, Default)]
pub(crate) struct NestedPrettierConfigs {
    /// The configuration Prettier uses for each directory that has one of its own, by its path relative to the
    /// component's root, in a stable order.
    pub configs: Vec<NestedPrettierConfig>,
    /// The problems that kept directories from being searched, such as an unreadable ignore file.
    pub errors: Vec<String>,
}

/// Find the Prettier configurations below `root`, a directory Prettier runs in, that take the place of the one in
/// `root` for the files below them. Prettier looks for its configuration from each file's directory up, using the
/// first `package.json` with a truthy `prettier` key or configuration file it finds, so each directory below `root`
/// that has one is listed, with the one Prettier uses there.
///
/// As Prettier formats them, hidden directories are searched, while `node_modules`, version control directories and
/// the paths `root`'s `.gitignore` and `.prettierignore` match are skipped. A configuration whose own path they match,
/// as `dist/**` does, is left out too, its directory taken to hold no file Prettier formats. Symlinked directories
/// aren't followed, so the search stays inside the project. A `package.json` that doesn't parse is taken to have no
/// `prettier` key.
pub(crate) fn find_nested_configs(root: &Path) -> NestedPrettierConfigs {
    let mut found = NestedPrettierConfigs::default();
    let ignored = Arc::new(ignore_matcher(root, &mut found.errors));
    let skipped = Arc::clone(&ignored);

    let walker = WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_dir())
                && (entry.depth() == 0
                    || !(IGNORED_DIRS.iter().any(|&dir| entry.file_name() == dir)
                        || skipped.matched(entry.path(), true).is_ignore()))
        })
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                found.errors.push(error.to_string());
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if let Some(config) = directory_config(entry.path(), relative)
            && !ignored.matched(root.join(config.path()), false).is_ignore()
        {
            found.configs.push(config);
        }
    }

    found
        .configs
        .sort_by(|a, b| a.directory().cmp(b.directory()));
    found.errors.sort();
    found.errors.dedup();
    found
}

impl NestedPrettierConfig {
    /// The directory the configuration applies to, relative to the component's root.
    pub(crate) fn directory(&self) -> &Path {
        let path = self.path();
        path.parent().unwrap_or(path)
    }

    /// The directory the configuration applies to, relative to the component's root, with `/` separators.
    pub(crate) fn directory_label(&self) -> String {
        slashed(self.directory())
    }

    /// The configuration's path relative to the component's root.
    pub(crate) fn path(&self) -> &Path {
        let (Self::PackageJson(path) | Self::File(path)) = self;
        path
    }

    /// The configuration's path relative to the component's root, with `/` separators.
    pub(crate) fn label(&self) -> String {
        slashed(self.path())
    }
}

/// The configuration Prettier uses for the files in `dir`, whose path relative to the component's root is `relative`,
/// if `dir` has one.
fn directory_config(dir: &Path, relative: &Path) -> Option<NestedPrettierConfig> {
    let package_json = dir.join("package.json");
    if package_json.is_file()
        && parse_json5_file::<PackageJson>(&package_json)
            .as_ref_ok()
            .is_some_and(PackageJson::configures_prettier)
    {
        return Some(NestedPrettierConfig::PackageJson(
            relative.join("package.json"),
        ));
    }
    PRETTIER_CONFIGS
        .iter()
        .find(|name| dir.join(name).is_file())
        .map(|name| NestedPrettierConfig::File(relative.join(name)))
}

/// The patterns of `root`'s ignore files, recording any that can't be read in `errors`.
fn ignore_matcher(root: &Path, errors: &mut Vec<String>) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    for name in IGNORE_FILES {
        let path = root.join(name);
        if path.is_file()
            && let Some(error) = builder.add(&path)
        {
            errors.push(format!("{name}: {error}"));
        }
    }
    builder.build().unwrap_or_else(|error| {
        errors.push(error.to_string());
        Gitignore::empty()
    })
}

/// `path` with its components joined by `/`.
fn slashed(path: &Path) -> String {
    path.iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// The labels of the nested configurations found in a scratch directory holding `files`, each a path and its
    /// contents.
    fn nested(files: &[(&str, &str)]) -> Vec<String> {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let found = find_nested_configs(dir.path());
        assert_eq!(found.errors, Vec::<String>::new());
        found
            .configs
            .iter()
            .map(NestedPrettierConfig::label)
            .collect()
    }

    #[test]
    fn the_roots_own_configuration_isnt_nested() {
        assert_eq!(
            nested(&[
                (".prettierrc", "{}"),
                ("package.json", r#"{"prettier": {}}"#),
                ("src/main.ts", ""),
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_directory_names_the_configuration_prettier_uses() {
        assert_eq!(
            nested(&[
                ("a/.prettierrc.json", "{}"),
                ("a/prettier.config.js", ""),
                ("b/package.json", r#"{"prettier": "@acme/prettier-config"}"#),
                ("b/.prettierrc", "{}"),
                ("c/package.json", r#"{"prettier": false}"#),
                ("c/.prettierrc.toml", ""),
                ("d/package.json", r#"{"name": "d"}"#),
                ("e/package.json", "not json"),
                (".github/deep/.prettierrc", "{}"),
            ]),
            [
                ".github/deep/.prettierrc",
                "a/.prettierrc.json",
                "b/package.json",
                "c/.prettierrc.toml",
            ]
        );
    }

    #[test]
    fn directories_prettier_skips_are_skipped() {
        assert_eq!(
            nested(&[
                (".gitignore", "/dist\n"),
                (
                    ".prettierignore",
                    "fixtures/\n!fixtures/kept/\ncoverage/**\nout/*\n"
                ),
                ("coverage/.prettierrc", "{}"),
                ("coverage/deep/.prettierrc", "{}"),
                ("out/package.json", r#"{"prettier": {}}"#),
                ("dist/.prettierrc", "{}"),
                ("fixtures/.prettierrc", "{}"),
                ("node_modules/pkg/package.json", r#"{"prettier": {}}"#),
                ("src/node_modules/.prettierrc", "{}"),
                (".git/.prettierrc", "{}"),
                (".jj/.prettierrc", "{}"),
                ("src/dist/.prettierrc", "{}"),
            ]),
            ["src/dist/.prettierrc"]
        );
    }

    #[test]
    fn an_invalid_ignore_pattern_is_reported_and_the_rest_apply() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".prettierignore"), "{a,b\nskipped/\n").unwrap();
        for sub in ["skipped", "kept"] {
            fs::create_dir_all(dir.path().join(sub)).unwrap();
            fs::write(dir.path().join(sub).join(".prettierrc"), "{}").unwrap();
        }
        let found = find_nested_configs(dir.path());
        assert_eq!(
            found.configs,
            [NestedPrettierConfig::File(PathBuf::from(
                "kept/.prettierrc"
            ))]
        );
        assert!(
            matches!(found.errors.as_slice(), [error] if error.starts_with(".prettierignore: ")),
            "{:?}",
            found.errors
        );
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The packages of a pnpm workspace.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde_json::Value;

use crate::config::{PackageJson, PnpmWorkspace};
use crate::context::{ConfigStatus, parse_json5_file};

/// The directories pnpm leaves out of a workspace whatever its patterns say, wherever they are
/// (`@pnpm/workspace.find-packages`).
const IGNORED_DIRS: [&str; 2] = ["node_modules", "bower_components"];

/// The packages of a pnpm workspace, other than the one at its root.
#[derive(Debug, Default)]
pub(crate) struct PnpmWorkspacePackages {
    /// Each package's manifest, keyed by the package's directory relative to the workspace root, in a stable order.
    pub packages: BTreeMap<PathBuf, ConfigStatus<PackageJson>>,
    /// The problems that kept packages from being found, such as an invalid pattern in `packages`.
    pub errors: Vec<String>,
}

/// Find the packages of the workspace whose `pnpm-workspace.yaml`, `workspace`, is in `root`, as pnpm does: each
/// `package.json` in a directory that a pattern in `packages` matches, unless a pattern starting with `!` matches it
/// too, or it lies in a `node_modules` or `bower_components` directory. Without `packages`, the workspace is the root
/// package alone. The root package itself isn't listed.
///
/// Unlike pnpm, this doesn't search hidden directories, which pnpm's patterns match when they name them, nor follow
/// symlinked directories, so the search stays inside the project, and it doesn't read the `package.yaml` and
/// `package.json5` manifests pnpm also reads. A pattern leading outside the workspace's directory is reported as an
/// error, and its packages aren't read.
pub(crate) fn find_packages(root: &Path, workspace: &PnpmWorkspace) -> PnpmWorkspacePackages {
    let mut found = PnpmWorkspacePackages::default();
    let Some(packages) = &workspace.packages else {
        return found;
    };
    let Some(patterns) = string_list(packages) else {
        found
            .errors
            .push(format!("packages is {packages}, not a list of strings"));
        return found;
    };

    let mut included = GlobSetBuilder::new();
    let mut excluded = GlobSetBuilder::new();
    for pattern in patterns {
        let (negated, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern), |pattern| (true, pattern));
        let Some(directory) = relative_pattern(pattern) else {
            let negation = if negated { "!" } else { "" };
            found.errors.push(format!(
                "Pattern \"{negation}{pattern}\" leads outside the workspace's directory, which isn't searched"
            ));
            continue;
        };
        let manifest = if directory.is_empty() {
            "package.json".to_owned()
        } else {
            format!("{directory}/package.json")
        };
        let set = if negated {
            &mut excluded
        } else {
            &mut included
        };
        add_glob(set, &manifest, &mut found.errors);
    }
    let (Some(included), Some(excluded)) = (
        build(&included, &mut found.errors),
        build(&excluded, &mut found.errors),
    ) else {
        return found;
    };

    let walker = WalkBuilder::new(root)
        .standard_filters(false)
        .hidden(true)
        .follow_links(false)
        .filter_entry(|entry| {
            let is_dir = entry
                .file_type()
                .is_some_and(|file_type| file_type.is_dir());
            !(is_dir
                && entry.depth() > 0
                && IGNORED_DIRS.iter().any(|&dir| entry.file_name() == dir))
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
        let is_file = entry
            .file_type()
            .is_some_and(|file_type| !file_type.is_dir());
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if !is_file
            || entry.file_name() != "package.json"
            || !included.is_match(relative)
            || excluded.is_match(relative)
        {
            continue;
        }
        let Some(directory) = relative.parent() else {
            continue;
        };
        if !directory.as_os_str().is_empty() {
            found.packages.insert(
                directory.to_path_buf(),
                parse_json5_file::<PackageJson>(entry.path()),
            );
        }
    }

    found.errors.sort();
    found.errors.dedup();
    found
}

/// The strings of `value`, if it is a list of nothing else.
fn string_list(value: &Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(Value::as_str).collect()
}

/// The directory pattern `pattern` names relative to the workspace root, without a leading `./` or trailing `/`, or
/// `None` if it may lead outside it, through `..` or an absolute path.
fn relative_pattern(pattern: &str) -> Option<String> {
    let path = Path::new(pattern);
    let parts = path
        .components()
        .map(|part| match part {
            Component::Normal(part) => part.to_str(),
            Component::CurDir => Some(""),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Add `glob` to `set`, with `*` not crossing a `/`, as in pnpm's patterns, recording it in `errors` if it is invalid.
fn add_glob(set: &mut GlobSetBuilder, glob: &str, errors: &mut Vec<String>) {
    match GlobBuilder::new(glob).literal_separator(true).build() {
        Ok(glob) => {
            set.add(glob);
        }
        Err(error) => errors.push(format!("Invalid pattern: {error}")),
    }
}

/// Build `set`, recording the error in `errors` if it can't be.
fn build(set: &GlobSetBuilder, errors: &mut Vec<String>) -> Option<GlobSet> {
    set.build()
        .map_err(|error| errors.push(format!("Invalid pattern: {error}")))
        .ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// The packages found in a scratch project holding a `package.json` in each of `directories`, for the given
    /// `packages` value.
    fn packages(directories: &[&str], packages: Option<Value>) -> (Vec<String>, Vec<String>) {
        let dir = TempDir::new().unwrap();
        for directory in directories {
            let path = dir.path().join(directory);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("package.json"), "{}").unwrap();
        }
        let workspace = PnpmWorkspace {
            packages,
            ..PnpmWorkspace::default()
        };
        let found = find_packages(dir.path(), &workspace);
        let paths = found
            .packages
            .keys()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        (paths, found.errors)
    }

    fn patterns(patterns: &[&str]) -> Value {
        Value::from(patterns.to_vec())
    }

    #[test]
    fn without_packages_the_workspace_is_the_root_alone() {
        let (found, errors) = packages(&[".", "packages/ui"], None);
        assert_eq!(found, Vec::<String>::new());
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn matches_patterns_as_pnpm_does() {
        let (found, errors) = packages(
            &[
                ".",
                "packages/ui",
                "packages/ui/nested",
                "apps/web",
                "apps/web/inner",
                "apps/legacy",
                "apps/legacy/inner",
                "apps/web/tests/e2e",
                "apps/web/bower_components/dep",
                "apps/web/node_modules/dep",
                "apps/.hidden",
                "tools/unlisted",
            ],
            Some(patterns(&["./packages/*/", "apps/**", "!apps/legacy", "."])),
        );
        assert_eq!(
            found,
            [
                "apps/legacy/inner",
                "apps/web",
                "apps/web/inner",
                "apps/web/tests/e2e",
                "packages/ui"
            ]
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn reports_patterns_it_cannot_follow() {
        let (found, errors) = packages(
            &["packages/ui"],
            Some(patterns(&["../shared/*", "packages/[ui"])),
        );
        assert_eq!(found, Vec::<String>::new());
        assert_eq!(errors.len(), 2, "{errors:?}");

        let (_, shape_errors) = packages(&[], Some(Value::from("packages/*")));
        assert_eq!(
            shape_errors,
            ["packages is \"packages/*\", not a list of strings"]
        );
    }
}

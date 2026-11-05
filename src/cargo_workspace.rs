// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The members of a Cargo workspace.

use std::collections::BTreeMap;
use std::path::{self, Component, Path, PathBuf};

use glob::Pattern;

use crate::config::{CargoDependency, CargoToml};
use crate::context::{ConfigStatus, parse_toml_file};

/// The members of a Cargo workspace, other than the package in its root manifest.
#[derive(Debug, Default)]
pub(crate) struct CargoWorkspaceMembers {
    /// Each member's manifest, keyed by the member's directory relative to the workspace root, in a stable order. A
    /// member whose directory has no `Cargo.toml` is `NotFound`, which Cargo rejects.
    pub members: BTreeMap<PathBuf, ConfigStatus<CargoToml>>,
    /// The problems that kept members from being found or read, such as an invalid pattern in `members`.
    pub errors: Vec<String>,
}

impl CargoWorkspaceMembers {
    /// The members whose manifests parsed, with their directories relative to the workspace root.
    pub(crate) fn parsed(&self) -> impl Iterator<Item = (&Path, &CargoToml)> {
        self.members
            .iter()
            .filter_map(|(path, status)| Some((path.as_path(), status.as_ref_ok()?)))
    }
}

/// Find the members of the workspace whose root manifest, `manifest`, is in `root`, as Cargo does:
///
/// - each directory that an entry of `[workspace] members` names, or matches as a glob pattern (a pattern matching
///   nothing names its literal path, and matches that aren't directories are ignored);
/// - transitively, each package in the workspace's directory that the root package or a member depends on by path,
///   directly or through `[workspace.dependencies]`;
/// - except those in a directory that `[workspace] exclude` lists, unless a `members` entry names one of its parents
///   literally.
///
/// Like Cargo, this compares paths as written, resolving `.` and `..` but not symlinks, and a member is keyed by such a
/// path. The root package itself isn't listed. A manifest without a `[workspace]` table has no members. A member is
/// only read if it lies inside the workspace's directory, symlinks followed; any other is reported as an error.
pub(crate) fn find_members(root: &Path, manifest: &CargoToml) -> CargoWorkspaceMembers {
    let mut found = CargoWorkspaceMembers::default();
    let Some(workspace) = &manifest.workspace else {
        return found;
    };
    let (Ok(absolute_root), Ok(canonical_root)) = (path::absolute(root), root.canonicalize())
    else {
        found
            .errors
            .push(format!("Could not resolve {}", root.display()));
        return found;
    };
    let normalized_root = normalize(&absolute_root);

    let finder = MemberFinder {
        excluded: workspace
            .exclude
            .iter()
            .map(|exclude| normalize(&normalized_root.join(exclude)))
            .collect(),
        explicit: workspace
            .members
            .iter()
            .map(|member| normalize(&normalized_root.join(member)))
            .collect(),
        root: &normalized_root,
        canonical_root: &canonical_root,
        inherited: &workspace.dependencies,
    };

    let mut pending: Vec<PathBuf> = Vec::new();
    for member in &workspace.members {
        pending.extend(finder.expand(member, &mut found.errors));
    }
    pending.extend(finder.path_dependencies(&normalized_root, manifest));

    while let Some(dir) = pending.pop() {
        let Some(relative) = finder.member_path(&dir, &mut found.errors) else {
            continue;
        };
        if relative.as_os_str().is_empty() || found.members.contains_key(&relative) {
            continue;
        }

        let status = parse_toml_file::<CargoToml>(&dir.join("Cargo.toml"));
        if let ConfigStatus::Ok(member) = &status {
            pending.extend(finder.path_dependencies(&dir, member));
        }
        found.members.insert(relative, status);
    }

    found.errors.sort();
    found.errors.dedup();
    found
}

/// Resolve `.` and `..` in `path` without consulting the file system, so symlinks aren't followed, as Cargo does
/// (`cargo_util::paths::normalize_path`). std's `Path::normalize_lexically` will do this once it is stable.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(part);
            }
        }
    }
    normalized
}

/// The workspace being searched for members.
struct MemberFinder<'a> {
    /// The workspace root: absolute and normalized, but with symlinks unresolved.
    root: &'a Path,
    /// The workspace root, canonical.
    canonical_root: &'a Path,
    /// The directories `[workspace] exclude` lists, normalized.
    excluded: Vec<PathBuf>,
    /// The `[workspace] members` entries, taken literally and normalized.
    explicit: Vec<PathBuf>,
    /// The dependencies members inherit, whose paths are relative to the root.
    inherited: &'a BTreeMap<String, CargoDependency>,
}

impl MemberFinder<'_> {
    /// The directories a `[workspace] members` entry names, normalized.
    fn expand(&self, member: &str, errors: &mut Vec<String>) -> Vec<PathBuf> {
        let Some(root) = self.root.to_str() else {
            errors.push(format!("{} isn't valid UTF-8", self.root.display()));
            return Vec::new();
        };
        let pattern = format!("{}/{member}", Pattern::escape(root));

        let paths = match glob::glob(&pattern) {
            Ok(paths) => paths,
            Err(error) => {
                errors.push(format!("Invalid member pattern \"{member}\": {error}"));
                return Vec::new();
            }
        };

        let matches: Vec<PathBuf> = paths
            .filter_map(|path| path.map_err(|error| errors.push(error.to_string())).ok())
            .collect();

        if matches.is_empty() {
            vec![normalize(&self.root.join(member))]
        } else {
            matches
                .iter()
                .filter(|path| path.is_dir())
                .map(|path| normalize(path))
                .collect()
        }
    }

    /// The directories of the packages that `manifest`, in `dir`, depends on by path inside the workspace,
    /// normalized. Those outside it aren't members.
    fn path_dependencies(&self, dir: &Path, manifest: &CargoToml) -> Vec<PathBuf> {
        manifest
            .package_dependency_tables()
            .flat_map(|(_, dependencies)| dependencies)
            .filter_map(|(name, dependency)| {
                if dependency.is_inherited() {
                    Some(self.root.join(self.inherited.get(name)?.path()?))
                } else {
                    Some(dir.join(dependency.path()?))
                }
            })
            .map(|path| normalize(&path))
            .filter(|path| path.starts_with(self.root))
            .collect()
    }

    /// The path of a member's directory, normalized, relative to the root, or `None` if it isn't a member after all
    /// because it is excluded, or if it can't be read because it lies outside the workspace's directory, which is
    /// recorded as an error.
    fn member_path(&self, dir: &Path, errors: &mut Vec<String>) -> Option<PathBuf> {
        let is_excluded = self
            .excluded
            .iter()
            .any(|excluded| dir.starts_with(excluded))
            && !self
                .explicit
                .iter()
                .any(|explicit| dir.starts_with(explicit));
        if is_excluded {
            return None;
        }

        // A directory that doesn't exist is reported as a member without a manifest.
        let resolves_inside = dir
            .canonicalize()
            .map_or(true, |resolved| resolved.starts_with(self.canonical_root));

        match dir.strip_prefix(self.root) {
            Ok(relative) if resolves_inside => Some(relative.to_path_buf()),
            _ => {
                errors.push(format!(
                    "{} is outside the workspace's directory, so it isn't checked",
                    dir.display()
                ));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// A scratch workspace with the given root manifest and files, each at a path relative to its root. A
    /// `Cargo.toml` without content of its own names a package.
    fn workspace(manifest: &str, files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();

        for &(file, content) in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let content = if content.is_empty() && path.ends_with("Cargo.toml") {
                "[package]\nname = \"member\"\n"
            } else {
                content
            };
            fs::write(path, content).unwrap();
        }

        dir
    }

    /// The members found, as their paths with `!` marking a manifest that isn't valid, and the errors.
    fn members(dir: &TempDir) -> (Vec<String>, Vec<String>) {
        let manifest =
            toml::from_str(&fs::read_to_string(dir.path().join("Cargo.toml")).unwrap()).unwrap();
        let found = find_members(dir.path(), &manifest);

        let members = found
            .members
            .iter()
            .map(|(path, status)| {
                let mark = if status.as_ref_ok().is_some() {
                    ""
                } else {
                    "!"
                };
                format!("{}{mark}", path.display())
            })
            .collect();
        (members, found.errors)
    }

    #[test]
    fn a_package_without_a_workspace_has_no_members() {
        let dir = workspace(
            "[package]\nname = \"x\"\n\n[dependencies]\nlocal = { path = \"local\" }\n",
            &[("local/Cargo.toml", "")],
        );

        assert_eq!(members(&dir), (vec![], vec![]));
    }

    #[test]
    fn expands_member_patterns() {
        let dir = workspace(
            "[workspace]\nmembers = [\"crates/*\", \"tools/cli\", \"missing\"]\n",
            &[
                ("crates/a/Cargo.toml", ""),
                ("crates/b/Cargo.toml", "not toml ["),
                ("crates/README.md", "Not a member."),
                ("crates/empty/.keep", ""),
                ("tools/cli/Cargo.toml", ""),
                ("tools/other/Cargo.toml", ""),
            ],
        );

        assert_eq!(
            members(&dir),
            (
                [
                    "crates/a",
                    "crates/b!",
                    "crates/empty!",
                    "missing!",
                    "tools/cli"
                ]
                .map(String::from)
                .to_vec(),
                vec![]
            )
        );
    }

    #[test]
    fn patterns_matching_only_files_name_no_members() {
        let dir = workspace(
            "[workspace]\nmembers = [\"crates/*\", \"gone/../tools/cli\"]\n",
            &[
                ("crates/README.md", "No members yet."),
                ("tools/cli/Cargo.toml", ""),
            ],
        );

        assert_eq!(members(&dir), (vec!["tools/cli".to_owned()], vec![]));
    }

    #[test]
    fn excludes_directories_unless_named_explicitly() {
        let dir = workspace(
            "[workspace]\nmembers = [\"crates/*\", \"crates/kept/inner\"]\nexclude = [\"crates/skipped\", \"crates/kept\"]\n",
            &[
                ("crates/a/Cargo.toml", ""),
                ("crates/skipped/Cargo.toml", ""),
                ("crates/kept/Cargo.toml", ""),
                ("crates/kept/inner/Cargo.toml", ""),
            ],
        );

        assert_eq!(
            members(&dir),
            (
                vec!["crates/a".to_owned(), "crates/kept/inner".to_owned()],
                vec![]
            )
        );
    }

    #[test]
    fn path_dependencies_in_the_workspace_are_members() {
        let dir = workspace(
            r#"
            [package]
            name = "root"

            [dependencies]
            direct = { path = "libs/direct" }
            excluded = { path = "libs/excluded" }

            [workspace]
            members = ["app"]
            exclude = ["libs/excluded"]

            [workspace.dependencies]
            shared = { path = "libs/shared" }
            "#,
            &[
                (
                    "app/Cargo.toml",
                    "[package]\nname = \"app\"\n\n[dependencies]\nmissing = { path = \"../libs/missing\" }\n\n\
                     [target.'cfg(unix)'.dev-dependencies]\nshared.workspace = true\n",
                ),
                (
                    "libs/shared/Cargo.toml",
                    "[package]\nname = \"shared\"\n\n[build-dependencies]\nnested = { path = \"../nested\" }\n",
                ),
                ("libs/nested/Cargo.toml", ""),
                ("libs/direct/Cargo.toml", ""),
                ("libs/excluded/Cargo.toml", ""),
                ("libs/unused/Cargo.toml", ""),
            ],
        );

        assert_eq!(
            members(&dir),
            (
                [
                    "app",
                    "libs/direct",
                    "libs/missing!",
                    "libs/nested",
                    "libs/shared"
                ]
                .map(String::from)
                .to_vec(),
                vec![]
            )
        );
    }

    #[test]
    fn path_dependencies_outside_the_workspace_are_not_members() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("Cargo.toml"),
            "[package]\nname = \"outside\"\n",
        )
        .unwrap();
        let dir = workspace(
            &format!(
                "[workspace]\nmembers = [\"app\"]\n\n[workspace.dependencies]\noutside = {{ path = {:?} }}\n",
                outside.path()
            ),
            &[(
                "app/Cargo.toml",
                "[package]\nname = \"app\"\n\n[dependencies]\noutside.workspace = true\n",
            )],
        );

        assert_eq!(members(&dir), (vec!["app".to_owned()], vec![]));
    }

    #[test]
    fn reports_members_it_cannot_read() {
        let dir = workspace(
            "[workspace]\nmembers = [\"crates/[\", \"../elsewhere\"]\n",
            &[],
        );

        let (members, errors) = members(&dir);
        assert_eq!(members, Vec::<String>::new());
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors
                .iter()
                .any(|error| error.starts_with("Invalid member pattern \"crates/[\""))
        );
        assert!(errors.iter().any(|error| {
            error.ends_with("is outside the workspace's directory, so it isn't checked")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_only_to_read_members() {
        use std::os::unix::fs::symlink;

        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("Cargo.toml"),
            "[package]\nname = \"outside\"\n",
        )
        .unwrap();
        // The excluded directory is reached through a symlink that isn't excluded, so it is a member there.
        let dir = workspace(
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"real\"]\n\n\
             [workspace.dependencies]\naway = { path = \"linked\" }\n",
            &[(
                "real/Cargo.toml",
                "[package]\nname = \"real\"\n\n[dependencies]\naway.workspace = true\n",
            )],
        );
        fs::create_dir_all(dir.path().join("crates")).unwrap();
        symlink(outside.path(), dir.path().join("crates/away")).unwrap();
        symlink(dir.path().join("real"), dir.path().join("crates/inside")).unwrap();
        symlink(outside.path(), dir.path().join("linked")).unwrap();

        let (members, errors) = members(&dir);
        assert_eq!(members, ["crates/inside"]);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors.iter().all(|error| {
            error.ends_with("is outside the workspace's directory, so it isn't checked")
        }));
    }
}

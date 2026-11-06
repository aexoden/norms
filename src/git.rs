// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The project's git repository, read through the `git` command.

use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use semver::Version;

/// The project's release tags, `vX.Y.Z` with an optional pre-release or build suffix, sorted by name. Only a project
/// at the top of a git working tree has any: a directory inside another repository, such as a test fixture, doesn't
/// share its tags. If git isn't installed or won't read the repository, there are none.
pub(crate) fn release_tags(path: &Path) -> Vec<String> {
    let Some(top) = git(path, &["rev-parse", "--show-toplevel"]) else {
        return Vec::new();
    };
    let is_top = match (
        Path::new(top.trim_end()).canonicalize(),
        path.canonicalize(),
    ) {
        (Ok(top), Ok(path)) => top == path,
        _ => false,
    };
    if !is_top {
        return Vec::new();
    }

    git(
        path,
        &[
            "for-each-ref",
            "--format=%(refname:strip=2)",
            "refs/tags/v*",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter(|tag| {
        tag.strip_prefix('v')
            .is_some_and(|version| Version::parse(version).is_ok())
    })
    .map(str::to_owned)
    .collect()
}

/// The output of `git -C <path> <args>`, if git ran and succeeded and its output is UTF-8.
fn git(path: &Path, args: &[&str]) -> Option<String> {
    output(git_command()?.arg("-C").arg(path).args(args))
}

/// A `git` command with the environment variables that point git at a particular repository, such as `GIT_DIR` and
/// `GIT_INDEX_FILE`, cleared, as git itself does before running in a submodule. The repository is then always the one
/// `-C` names, even when norms runs inside a git hook, which sets them for the hook's own repository.
fn git_command() -> Option<Command> {
    /// The variables to clear, as `git rev-parse --local-env-vars` lists them (cached).
    static LOCAL_ENV_VARS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        output(Command::new("git").args(["rev-parse", "--local-env-vars"]))
            .map(|vars| vars.lines().map(str::to_owned).collect())
    });

    let mut command = Command::new("git");
    for var in LOCAL_ENV_VARS.as_ref()? {
        command.env_remove(var);
    }
    Some(command)
}

/// The output of `command`, if it ran and succeeded and its output is UTF-8.
fn output(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Run git in `path` with a fixed identity and no signing, hooks or user configuration, so the developer's own
    /// settings can't interfere.
    fn run_git(path: &Path, args: &[&str]) {
        let status = git_command()
            .unwrap()
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=norms",
                "-c",
                "user.email=norms@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn reads_release_tags_at_the_top_of_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_git(root, &["init", "--quiet"]);
        run_git(root, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
        for tag in ["v1.0.0", "v0.2.0-rc.1", "v1.0", "release-1.0.0", "v1.x"] {
            run_git(root, &["tag", tag]);
        }
        run_git(root, &["tag", "-a", "v2.0.0", "-m", "v2.0.0"]);

        assert_eq!(release_tags(root), ["v0.2.0-rc.1", "v1.0.0", "v2.0.0"]);

        // A directory inside the repository doesn't share its tags.
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(release_tags(&sub), Vec::<String>::new());
    }

    #[test]
    fn clears_the_variables_selecting_a_repository() {
        let command = git_command().unwrap();
        let cleared: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(var, _)| var.to_string_lossy().into_owned())
            .collect();
        for var in [
            "GIT_DIR",
            "GIT_INDEX_FILE",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
        ] {
            assert!(cleared.iter().any(|cleared| cleared == var), "{var}");
        }
    }

    #[test]
    fn finds_no_tags_outside_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(release_tags(dir.path()), Vec::<String>::new());
        assert_eq!(
            release_tags(&dir.path().join("missing")),
            Vec::<String>::new()
        );
    }
}

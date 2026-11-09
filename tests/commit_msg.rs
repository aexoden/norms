// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms commit-msg` as the hook does.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    /// The variables git sets for a hook, or reads configuration from, that could point it at this repository or the
    /// developer's settings when the tests run inside a hook.
    const GIT_VARS: [&str; 8] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_EDITOR",
    ];

    /// Runs `norms commit-msg` on `file` from its directory, outside any repository and without the developer's git
    /// configuration, with the variables `env` sets.
    fn commit_msg_with(file: &Path, env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_norms"));
        for var in GIT_VARS {
            command.env_remove(var);
        }
        command
            .arg("commit-msg")
            .arg(file)
            .current_dir(file.parent().unwrap())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CEILING_DIRECTORIES", file.parent().unwrap())
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    fn commit_msg(file: &Path) -> Output {
        commit_msg_with(file, &[])
    }

    /// The lines `norms commit-msg` reports as too long, by number.
    fn long_lines(output: &Output) -> Vec<usize> {
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| line.split(':').nth(1)?.parse().ok())
            .collect()
    }

    #[test]
    fn exits_with_failure_for_a_long_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("COMMIT_EDITMSG");

        fs::write(&file, "feat: add it\n\nA short body.\n").unwrap();
        let passed = commit_msg(&file);
        assert!(passed.status.success());
        assert_eq!(String::from_utf8_lossy(&passed.stderr), "");

        fs::write(&file, format!("feat: {}\n", "a".repeat(67))).unwrap();
        let failed = commit_msg(&file);
        assert_eq!(failed.status.code(), Some(1));
        let stderr = String::from_utf8(failed.stderr).unwrap();
        assert!(
            stderr.contains(":1: subject is 73 characters long, over 72"),
            "{stderr}"
        );
    }

    #[test]
    fn keeps_comment_lines_when_git_opens_no_editor() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("COMMIT_EDITMSG");
        fs::write(&file, format!("feat: add it\n\n# {}\n", "a".repeat(80))).unwrap();

        assert!(commit_msg(&file).status.success());
        let kept = commit_msg_with(&file, &[("GIT_EDITOR", ":")]);
        assert_eq!(kept.status.code(), Some(1));
        assert_eq!(long_lines(&kept), [3]);
        // Another editor means the message was edited.
        assert!(
            commit_msg_with(&file, &[("GIT_EDITOR", "vi")])
                .status
                .success()
        );
    }

    #[test]
    fn leaves_out_lines_starting_with_the_configured_comment_string() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("COMMIT_EDITMSG");
        fs::write(
            &file,
            format!(
                "feat: add it\n\n# {}\n; {}\n",
                "a".repeat(80),
                "b".repeat(80)
            ),
        )
        .unwrap();

        let config = |key, value| {
            commit_msg_with(
                &file,
                &[
                    ("GIT_CONFIG_COUNT", "1"),
                    ("GIT_CONFIG_KEY_0", key),
                    ("GIT_CONFIG_VALUE_0", value),
                ],
            )
        };
        assert_eq!(long_lines(&commit_msg(&file)), [4]);
        assert_eq!(long_lines(&config("core.commentChar", ";")), [3]);
        assert_eq!(long_lines(&config("core.commentString", ";")), [3]);
        assert_eq!(long_lines(&config("core.commentChar", "auto")), [4]);
    }

    /// Runs `git` in `repo` with `args`, without the developer's configuration and with norms as its commit-msg hook,
    /// returning whether it succeeded.
    fn git(repo: &Path, args: &[&str], editor: Option<&str>) -> bool {
        let mut command = Command::new("git");
        for var in GIT_VARS {
            command.env_remove(var);
        }
        if let Some(editor) = editor {
            command.env("GIT_EDITOR", editor);
        }
        command
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=norms",
                "-c",
                "user.email=norms@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=hooks",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
            .status
            .success()
    }

    #[cfg(unix)]
    #[test]
    fn git_runs_the_hook_as_norms_expects() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        assert!(git(repo, &["init", "--quiet"], None));
        let hooks = repo.join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("commit-msg");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexec '{}' commit-msg \"$1\"\n",
                env!("CARGO_BIN_EXE_norms")
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

        let long = format!("# {}", "a".repeat(80));
        let commit = &["commit", "--quiet", "--allow-empty"];
        // git keeps the comment line of a message given with -m, so the hook rejects it.
        assert!(!git(
            repo,
            &[commit.as_slice(), &["-m", "feat: add it", "-m", &long]].concat(),
            None
        ));
        // git leaves out the comment line of a message from the editor, and so does the hook.
        let editor = format!("printf 'feat: add it\\n\\n%s\\n' '{long}' >");
        assert!(git(repo, commit, Some(&editor)));
    }

    #[test]
    fn exits_with_failure_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = commit_msg(&dir.path().join("missing"));
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("Error: Failed to read "), "{stderr}");
    }
}

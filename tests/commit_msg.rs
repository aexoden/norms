// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms commit-msg` as the hook does.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    fn commit_msg(file: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_norms"))
            .arg("commit-msg")
            .arg(file)
            .output()
            .unwrap()
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
    fn exits_with_failure_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = commit_msg(&dir.path().join("missing"));
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("Error: Failed to read "), "{stderr}");
    }
}

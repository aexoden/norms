// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms commit-msg` as the hook does, and checks that the hook norms
//! ships matches the local copy norms runs on itself.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use serde_json::Value;

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

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

    fn yaml(path: &str) -> Value {
        serde_saphyr::from_str(&fs::read_to_string(root().join(path)).unwrap()).unwrap()
    }

    /// The value of `key` in a YAML mapping, or null.
    fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
        value.get(key).unwrap_or(&Value::Null)
    }

    #[test]
    fn local_hook_matches_the_shipped_one() {
        let manifest = yaml(".pre-commit-hooks.yaml");
        let shipped = manifest
            .as_array()
            .unwrap()
            .iter()
            .find(|hook| get(hook, "id") == "norms-commit-msg")
            .expect("the shipped hook");
        let config = yaml(".pre-commit-config.yaml");
        let local = get(&config, "repos")
            .as_array()
            .unwrap()
            .iter()
            .filter(|repo| get(repo, "repo") == "local")
            .flat_map(|repo| get(repo, "hooks").as_array().unwrap())
            .find(|hook| get(hook, "id") == "norms-commit-msg")
            .expect("the local hook");

        for key in ["name", "description", "stages"] {
            assert!(!get(shipped, key).is_null(), "{key}");
            assert_eq!(get(shipped, key), get(local, key), "{key}");
        }
        assert_eq!(get(shipped, "entry"), "norms commit-msg");
        assert_eq!(
            get(local, "entry"),
            "cargo run --quiet --locked -- commit-msg"
        );
    }
}

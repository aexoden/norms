// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms verify` to check its exit code, on a copy of a conforming
//! fixture that is changed to make a check warn.

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// Copies the directory `from` to `to`.
    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn verify(path: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_norms"))
            .arg("verify")
            .args(args)
            .arg(path)
            .output()
            .unwrap()
    }

    /// Writes a `norms.toml` waiving each of `checks`.
    fn waive(project: &Path, checks: &[&str]) {
        let mut waivers = String::new();
        for check in checks {
            writeln!(
                waivers,
                "[[waivers]]\ncheck = \"{check}\"\nreason = \"Test.\""
            )
            .unwrap();
        }
        fs::write(project.join("norms.toml"), waivers).unwrap();
    }

    fn stderr(output: Output) -> String {
        String::from_utf8(output.stderr).unwrap()
    }

    #[test]
    fn deny_warnings_fails_on_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        copy_dir(&root().join("tests/fixtures/rust"), &project);

        let conforming = verify(&project, &["--deny-warnings"]);
        assert!(conforming.status.success(), "{conforming:?}");

        // A waiver whose check passes only warns.
        waive(&project, &["readme-exists"]);
        let lenient = verify(&project, &[]);
        assert!(lenient.status.success(), "{lenient:?}");

        let denied = verify(&project, &["--deny-warnings"]);
        assert_eq!(denied.status.code(), Some(1));
        assert_eq!(
            stderr(denied),
            "Error: 1 warning, treated as a failure by --deny-warnings.\n"
        );

        let json = verify(&project, &["--json", "--deny-warnings"]);
        assert_eq!(json.status.code(), Some(1));
        let output: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(output.pointer("/summary/warnings").unwrap(), 1);

        waive(&project, &["readme-exists", "license-exists"]);
        assert_eq!(
            stderr(verify(&project, &["--deny-warnings"])),
            "Error: 2 warnings, treated as failures by --deny-warnings.\n"
        );

        // A failure needs no explanation of the exit code.
        fs::remove_file(project.join(".gitignore")).unwrap();
        let failed = verify(&project, &["--deny-warnings"]);
        assert_eq!(failed.status.code(), Some(1));
        assert_eq!(stderr(failed), "");
    }

    #[test]
    fn deny_warnings_ignores_waived_checks() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        copy_dir(&root().join("tests/fixtures/rust"), &project);

        fs::remove_file(project.join("renovate.json")).unwrap();
        waive(&project, &["renovate-exists"]);
        let waived = verify(&project, &["--deny-warnings"]);
        assert!(waived.status.success(), "{waived:?}");
    }
}

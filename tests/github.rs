// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms github` to check that it exits with status 2, before reading GitHub, when it can't check a project.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Output;

    fn github(path: &Path) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_norms"))
            .args(["github", "--repo", "owner/name"])
            .arg(path)
            .output()
            .unwrap()
    }

    fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    #[test]
    fn a_path_that_isnt_a_directory_exits_2() {
        let dir = tempfile::tempdir().unwrap();
        let output = github(&dir.path().join("missing"));
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(stderr(&output).contains("is not a valid directory"));
    }

    #[test]
    fn an_invalid_norms_toml_exits_2() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("norms.toml"), "[[waivers]]\ncheck = 1\n").unwrap();
        let output = github(dir.path());
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(stderr(&output).contains("norms.toml is invalid"));
    }
}

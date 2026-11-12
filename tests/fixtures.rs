// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms verify` against each project in `tests/fixtures/` and compares
//! its findings with the snapshot of the same name in `tests/snapshots/`.
//!
//! Run with `NORMS_BLESS=1` to rewrite the snapshots from the current output,
//! then review the changes with `git diff`.
//!
//! Run with `NORMS_RENDER=1` to first re-render the language fixtures from the
//! template in the working tree, as `tests/README.md` describes. With both, the
//! snapshots are then rewritten from the new renders.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use serde_json::Value;

    /// The language fixtures and the answers each is rendered with, beside
    /// `RENDER_ANSWERS`.
    const RENDERS: [(&str, &[&str]); 5] = [
        ("rust", &["project_name=demo-rust", "primary_language=rust"]),
        (
            "python",
            &["project_name=demo-python", "primary_language=python"],
        ),
        (
            "typescript",
            &[
                "project_name=demo-typescript",
                "primary_language=typescript",
            ],
        ),
        (
            "rust-typescript",
            &[
                "project_name=demo",
                "primary_language=rust",
                r#"secondary_languages=["typescript"]"#,
                "typescript_path=frontend",
            ],
        ),
        (
            "python-rust-library",
            &[
                "project_name=demo-library",
                "primary_language=python",
                "project_type=library",
                r#"secondary_languages=["rust"]"#,
            ],
        ),
    ];

    /// The answers every language fixture is rendered with.
    const RENDER_ANSWERS: [&str; 1] = ["description=A demo."];

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn env_is_set(name: &str) -> bool {
        std::env::var_os(name).is_some_and(|v| v == "1")
    }

    /// The Copier version CI pins, from `COPIER_VERSION` in `ci.yaml`'s `env`.
    fn copier_version() -> String {
        let workflow: Value = serde_saphyr::from_str(
            &fs::read_to_string(root().join(".github/workflows/ci.yaml")).unwrap(),
        )
        .unwrap();
        workflow
            .pointer("/env/COPIER_VERSION")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    }

    /// Renders the fixture `name` into `dir` with the Copier version CI pins,
    /// skipping its tasks. `--vcs-ref HEAD` renders the last commit with any
    /// uncommitted changes, untracked files included. The answers file is
    /// dropped, and the lock files the tasks would generate are written (see
    /// `write_lock_files`).
    fn render(name: &str, answers: &[&str], copier: &str, dir: &Path) {
        let mut command = Command::new("uvx");
        command.args([
            copier,
            "copy",
            "--quiet",
            "--trust",
            "--defaults",
            "--vcs-ref",
            "HEAD",
            "--skip-tasks",
        ]);
        for answer in RENDER_ANSWERS.iter().chain(answers) {
            command.args(["--data", answer]);
        }
        let status = command
            .arg(root())
            .arg(dir)
            .status()
            .unwrap_or_else(|e| panic!("{name}: can't run uvx to render the fixture: {e}"));
        assert!(status.success(), "{name}: Copier failed ({status})");

        fs::remove_file(dir.join(".copier-answers.yml")).unwrap();
        write_lock_files(dir);
    }

    /// Re-renders every language fixture. All are rendered before any is
    /// replaced, so that the checkout Copier renders from stays as it was,
    /// and a failed render leaves the fixtures as they were.
    fn render_all() {
        let copier = format!("copier@{}", copier_version());
        let renders = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        for (name, answers) in RENDERS {
            render(name, answers, &copier, &renders.path().join(name));
        }
        for (name, _) in RENDERS {
            let fixture = root().join("tests/fixtures").join(name);
            if fixture.exists() {
                fs::remove_dir_all(&fixture).unwrap();
            }
            copy_dir(&renders.path().join(name), &fixture);
        }
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

    /// Writes the lock file of each component in `dir` and its subdirectories,
    /// skipping hidden ones: `Cargo.lock` as Cargo generates it, and stubs of
    /// `uv.lock` and `pnpm-lock.yaml`, since the checks only look for them and
    /// real ones are slow to generate and large.
    fn write_lock_files(dir: &Path) {
        if dir.join("Cargo.toml").is_file() {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
            let status = Command::new(cargo)
                .arg("generate-lockfile")
                .arg("--quiet")
                .arg("--manifest-path")
                .arg(dir.join("Cargo.toml"))
                .status()
                .unwrap();
            assert!(
                status.success(),
                "{}: cargo generate-lockfile failed",
                dir.display()
            );
        }

        let pyproject = dir.join("pyproject.toml");
        if pyproject.is_file() {
            let pyproject: toml::Table = fs::read_to_string(pyproject).unwrap().parse().unwrap();
            let requires_python = pyproject
                .get("project")
                .and_then(|project| project.get("requires-python"))
                .and_then(toml::Value::as_str)
                .unwrap();
            fs::write(
                dir.join("uv.lock"),
                format!("version = 1\nrevision = 3\nrequires-python = \"{requires_python}\"\n"),
            )
            .unwrap();
        }

        if dir.join("package.json").is_file() {
            fs::write(dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        }

        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if entry.file_type().unwrap().is_dir() && !hidden {
                write_lock_files(&entry.path());
            }
        }
    }

    fn snapshots_dir() -> PathBuf {
        root().join("tests/snapshots")
    }

    /// Returns the name of every fixture project, in order.
    fn fixtures() -> BTreeSet<String> {
        fs::read_dir(root().join("tests/fixtures"))
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_type().unwrap().is_dir())
            .map(|entry| entry.file_name().into_string().unwrap())
            .collect()
    }

    /// Returns the name of every snapshot, in order, ignoring other files.
    fn snapshots() -> BTreeSet<String> {
        fs::read_dir(snapshots_dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter_map(|file| file.strip_suffix(".txt").map(str::to_owned))
            .collect()
    }

    /// Runs norms on a fixture and renders what it reports: the exit code, the
    /// detected languages and components, then a line per result with its
    /// status, ID, component in brackets (if it has one) and name, followed by
    /// its message, if any, indented beneath. The project's path is left out,
    /// since it varies by checkout.
    fn verify(fixture: &str) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_norms"))
            .arg("verify")
            .arg("--json")
            .arg(root().join("tests/fixtures").join(fixture))
            .output()
            .unwrap();
        let json: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "{fixture}: invalid JSON from norms ({e}): {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });

        let str_at = |value: &Value, key: &str| -> String {
            value.get(key).and_then(Value::as_str).unwrap().to_owned()
        };

        let mut rendered = String::new();
        let code = output
            .status
            .code()
            .unwrap_or_else(|| panic!("{fixture}: norms was killed by a signal"));
        writeln!(rendered, "exit: {code}").unwrap();

        let languages: Vec<&str> = json
            .get("languages")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect();
        let languages = if languages.is_empty() {
            "(none)".to_owned()
        } else {
            languages.join(", ")
        };
        writeln!(rendered, "languages: {languages}").unwrap();

        let components: Vec<String> = json
            .get("components")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|c| format!("{} ({})", str_at(c, "language"), str_at(c, "path")))
            .collect();
        let components = if components.is_empty() {
            "(none)".to_owned()
        } else {
            components.join(", ")
        };
        writeln!(rendered, "components: {components}").unwrap();
        writeln!(rendered).unwrap();

        for result in json.get("results").and_then(Value::as_array).unwrap() {
            let status = str_at(result, "status");
            let id = str_at(result, "id");
            let name = str_at(result, "name");
            let component = result
                .get("component")
                .and_then(Value::as_str)
                .map(|path| format!("[{path}] "))
                .unwrap_or_default();
            writeln!(rendered, "{status:<4}  {id}  {component}{name}").unwrap();

            let message = result.get("message").and_then(Value::as_str);
            for line in message.into_iter().flat_map(str::lines) {
                writeln!(rendered, "{}", format!("      {line}").trim_end()).unwrap();
            }
        }

        rendered
    }

    #[test]
    fn fixtures_match_snapshots() {
        let bless = env_is_set("NORMS_BLESS");
        let rendered = env_is_set("NORMS_RENDER");
        if rendered {
            render_all();
        }

        let fixtures = fixtures();
        let mut problems = Vec::new();

        for name in &fixtures {
            let path = snapshots_dir().join(format!("{name}.txt"));
            let actual = verify(name);

            if bless {
                fs::write(&path, actual).unwrap();
                continue;
            }

            let Ok(expected) = fs::read_to_string(&path) else {
                problems.push(format!("{name}: no snapshot"));
                continue;
            };

            if expected != actual {
                let expected: Vec<&str> = expected.lines().collect();
                let actual: Vec<&str> = actual.lines().collect();
                let line = (0..expected.len().max(actual.len()))
                    .find(|&i| expected.get(i) != actual.get(i));
                problems.push(line.map_or_else(
                    || format!("{name}: differs only in line endings"),
                    |line| {
                        format!(
                            "{name}: line {}\n  expected: {}\n  actual:   {}",
                            line + 1,
                            expected.get(line).unwrap_or(&"<end>"),
                            actual.get(line).unwrap_or(&"<end>")
                        )
                    },
                ));
            }
        }

        for name in snapshots().difference(&fixtures) {
            if bless {
                fs::remove_file(snapshots_dir().join(format!("{name}.txt"))).unwrap();
            } else {
                problems.push(format!("{name}: snapshot without a fixture"));
            }
        }

        let (command, paths) = if rendered {
            (
                "NORMS_RENDER=1 NORMS_BLESS=1",
                "tests/fixtures tests/snapshots",
            )
        } else {
            ("NORMS_BLESS=1", "tests/snapshots")
        };
        assert!(
            problems.is_empty(),
            "fixtures differ from their snapshots:\n{}\n\nIf the changes are intended, run \
             `{command} cargo test --test fixtures` and review them with `git diff {paths}`.",
            problems.join("\n")
        );
    }
}

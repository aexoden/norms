// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Runs `norms verify` against each project in `tests/fixtures/` and compares
//! its findings with the snapshot of the same name in `tests/snapshots/`.
//!
//! Run with `NORMS_BLESS=1` to rewrite the snapshots from the current output,
//! then review the changes with `git diff`.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use serde_json::Value;

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
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
        let bless = std::env::var_os("NORMS_BLESS").is_some_and(|v| v == "1");
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

        assert!(
            problems.is_empty(),
            "fixtures differ from their snapshots:\n{}\n\nIf the changes are intended, run \
             `NORMS_BLESS=1 cargo test --test fixtures` and review them with `git diff tests/snapshots`.",
            problems.join("\n")
        );
    }
}

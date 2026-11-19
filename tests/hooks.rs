// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks that each hook norms ships in `.pre-commit-hooks.yaml` matches the
//! local copy norms runs on itself in `prek.toml`, that norms' `prek.toml`
//! keeps the template's settings, and that norms' CI checks commit messages as
//! the template's does.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use serde_json::{Map, Value};

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn read(path: &str) -> String {
        fs::read_to_string(root().join(path)).unwrap()
    }

    /// The value of `key` in a mapping, or null.
    fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
        value.get(key).unwrap_or(&Value::Null)
    }

    /// The keys of a hook other than `skip`, which differ between the copies.
    fn keys<'a>(hook: &'a Map<String, Value>, skip: &[&str]) -> BTreeSet<&'a str> {
        hook.keys()
            .map(String::as_str)
            .filter(|key| !skip.contains(key))
            .collect()
    }

    #[test]
    fn local_hooks_match_the_shipped_ones() {
        let shipped: Value = serde_saphyr::from_str(&read(".pre-commit-hooks.yaml")).unwrap();
        let config: Value = toml::from_str(&read("prek.toml")).unwrap();
        let local: Vec<&Value> = get(&config, "repos")
            .as_array()
            .unwrap()
            .iter()
            .filter(|repo| get(repo, "repo") == "local")
            .flat_map(|repo| get(repo, "hooks").as_array().unwrap())
            .collect();

        let shipped_ids: BTreeSet<&str> = shipped
            .as_array()
            .unwrap()
            .iter()
            .map(|hook| get(hook, "id").as_str().unwrap())
            .collect();
        assert_eq!(
            shipped_ids,
            BTreeSet::from(["norms-commit-msg", "norms-verify"])
        );

        for hook in shipped.as_array().unwrap() {
            let id = get(hook, "id").as_str().unwrap();
            let copy = local
                .iter()
                .find(|local| get(local, "id") == id)
                .unwrap_or_else(|| panic!("{id}: no local copy"));

            // A project chooses the hook's args and priority, so only the local
            // copy has them.
            assert_eq!(
                keys(hook.as_object().unwrap(), &["entry", "language"]),
                keys(
                    copy.as_object().unwrap(),
                    &["entry", "language", "args", "priority"]
                ),
                "{id}"
            );
            for key in ["name", "description", "stages"] {
                assert!(!get(hook, key).is_null(), "{id}: {key}");
            }
            for key in keys(hook.as_object().unwrap(), &["entry", "language"]) {
                assert_eq!(get(hook, key), get(copy, key), "{id}: {key}");
            }

            let command = get(hook, "entry")
                .as_str()
                .unwrap()
                .strip_prefix("norms ")
                .unwrap_or_else(|| panic!("{id}: the entry doesn't run norms"));
            assert_eq!(
                get(copy, "entry"),
                &format!("cargo run --quiet --locked -- {command}"),
                "{id}"
            );
            assert_eq!(get(hook, "language"), "rust", "{id}");
            assert_eq!(get(copy, "language"), "system", "{id}");
        }

        // norms treats its own warnings as failures.
        let verify = local
            .iter()
            .find(|local| get(local, "id") == "norms-verify")
            .unwrap();
        assert_eq!(get(verify, "args"), &serde_json::json!(["--deny-warnings"]));
    }

    #[test]
    fn settings_match_the_template() {
        let own: Value = toml::from_str(&read("prek.toml")).unwrap();
        // A render of the template, which the fixtures job keeps current.
        let render: Value = toml::from_str(&read("tests/fixtures/rust/prek.toml")).unwrap();

        for key in ["minimum_prek_version", "priorities"] {
            assert!(!get(&render, key).is_null(), "{key}");
            assert_eq!(get(&own, key), get(&render, key), "{key}");
        }
    }

    /// The step of a workflow at `path` named `Check the commit message`, in whichever job runs it.
    fn commit_msg_step(path: &str) -> Value {
        let workflow: Value = serde_saphyr::from_str(&read(path)).unwrap();
        let steps: Vec<&Value> = get(&workflow, "jobs")
            .as_object()
            .unwrap()
            .values()
            .flat_map(|job| get(job, "steps").as_array().unwrap())
            .filter(|step| get(step, "name") == "Check the commit message")
            .collect();
        assert_eq!(steps.len(), 1, "{path}");
        (*steps.first().unwrap()).clone()
    }

    #[test]
    fn ci_checks_commit_messages_as_the_template_does() {
        // The rust fixture is rendered from the template.
        assert_eq!(
            commit_msg_step(".github/workflows/ci.yaml"),
            commit_msg_step("tests/fixtures/rust/.github/workflows/ci.yaml")
        );
    }
}

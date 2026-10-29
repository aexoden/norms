// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Consistency checks between the template's `versions.yml` and this
//! repository's own workflows, which Renovate updates through different
//! managers.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use regex::Regex;
    use serde_json::Value;

    /// An action entry in `versions.yml`.
    struct TemplateAction {
        dep: String,
        var: String,
        value: String,
    }

    /// An action pinned by SHA in a workflow.
    struct WorkflowPin {
        file: String,
        dep: String,
        value: String,
    }

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    }

    /// Returns the pattern of the Renovate manager that updates the actions
    /// in `versions.yml`, so these checks see the entries Renovate sees.
    fn renovate_action_pattern() -> Regex {
        let config: Value = serde_json::from_str(&read(&root().join("renovate.json"))).unwrap();
        let manager = config
            .get("customManagers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|m| m.get("versioningTemplate").and_then(Value::as_str) == Some("github-actions"))
            .expect("renovate.json has a custom manager with github-actions versioning");
        let Some([pattern]) = manager
            .get("matchStrings")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
        else {
            panic!("the actions manager in renovate.json has exactly one matchString");
        };
        Regex::new(pattern.as_str().unwrap()).unwrap()
    }

    /// Returns each action entry in `versions.yml`, failing if Renovate
    /// would not see one of them.
    fn template_actions() -> Vec<TemplateAction> {
        let text = read(&root().join("versions.yml"));
        let entry = renovate_action_pattern();
        let var = Regex::new(r"\n(?<var>action_\w+):\n").unwrap();

        let actions: Vec<_> = entry
            .captures_iter(&text)
            .map(|c| TemplateAction {
                dep: c["depName"].to_owned(),
                var: var.captures(&c[0]).unwrap()["var"].to_owned(),
                value: format!("{} # {}", &c["currentDigest"], &c["currentValue"]),
            })
            .collect();

        let declared: Vec<_> = Regex::new(r"(?m)^action_\w+:")
            .unwrap()
            .find_iter(&text)
            .map(|m| m.as_str().trim_end_matches(':'))
            .collect();
        let unmatched: Vec<_> = declared
            .iter()
            .filter(|d| !actions.iter().any(|a| a.var == **d))
            .collect();
        assert!(
            unmatched.is_empty(),
            "these versions.yml entries don't match the actions manager's pattern in \
             renovate.json, so Renovate won't update them: {unmatched:?}"
        );
        actions
    }

    /// Returns each action pinned by SHA in this repository's workflows,
    /// failing on any SHA pin it can't read.
    fn workflow_pins() -> Vec<WorkflowPin> {
        let sha = Regex::new(r"@[0-9a-fA-F]{40}\b").unwrap();
        let uses = Regex::new(
            r"^\s*(?:- )?uses: (?<dep>[\w.-]+/[\w.-]+)(?:/[^@\s]*)?@(?<sha>[0-9a-f]{40}) # (?<version>\S+)$",
        )
        .unwrap();

        let mut paths: Vec<_> = fs::read_dir(root().join(".github/workflows"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();

        let mut pins = Vec::new();
        let mut unreadable = Vec::new();
        for path in paths {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            for (number, line) in read(&path).lines().enumerate() {
                if let Some(c) = uses.captures(line) {
                    pins.push(WorkflowPin {
                        file: file.clone(),
                        dep: c["dep"].to_owned(),
                        value: format!("{} # {}", &c["sha"], &c["version"]),
                    });
                } else if sha.is_match(line) {
                    unreadable.push(format!("{file}:{}: {}", number + 1, line.trim()));
                }
            }
        }
        assert!(
            unreadable.is_empty(),
            "write SHA-pinned actions as `uses: owner/repo@<sha> # <version>`:\n{}",
            unreadable.join("\n")
        );
        pins
    }

    #[test]
    fn template_actions_pin_a_sha_and_full_version() {
        let pinned = Regex::new(r"^[0-9a-f]{40} # v\d+\.\d+\.\d+$").unwrap();

        let invalid: Vec<_> = template_actions()
            .into_iter()
            .filter(|a| !pinned.is_match(&a.value))
            .map(|a| format!("{} ({}): \"{}\"", a.var, a.dep, a.value))
            .collect();

        assert!(
            invalid.is_empty(),
            "versions.yml actions must be \"<sha> # vX.Y.Z\":\n{}",
            invalid.join("\n")
        );
    }

    #[test]
    fn template_actions_are_unique() {
        let mut vars: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for action in template_actions() {
            vars.entry(action.dep).or_default().push(action.var);
        }
        vars.retain(|_, v| v.len() > 1);

        assert!(
            vars.is_empty(),
            "versions.yml lists these actions more than once: {vars:?}"
        );
    }

    #[test]
    fn template_actions_match_workflow_pins() {
        let actions = template_actions();
        let pins = workflow_pins();
        let mut problems = Vec::new();

        for action in &actions {
            let matching: Vec<_> = pins.iter().filter(|p| p.dep == action.dep).collect();
            if matching.is_empty() {
                problems.push(format!(
                    "{}: {} isn't pinned in any workflow, so nothing checks its depName",
                    action.var, action.dep
                ));
            }
            for pin in matching.into_iter().filter(|p| p.value != action.value) {
                problems.push(format!(
                    "{}: \"{}\", but {} has \"{}\"",
                    action.var, action.value, pin.file, pin.value
                ));
            }
        }

        assert!(
            problems.is_empty(),
            "versions.yml and the workflows disagree:\n{}",
            problems.join("\n")
        );
    }
}

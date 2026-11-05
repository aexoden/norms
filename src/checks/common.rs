// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects reqgardless of language.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{PrecommitHook, SHARED_PRESET, Workflow};
use crate::context::{AUDIT_WORKFLOW, CI_WORKFLOW, ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists, matches_regex};

//
// norms configuration
//

/// Check `norms.toml`, which is optional: most projects have no waivers or exclusions.
pub(crate) fn check_norms_config(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.norms();

    if matches!(status, ConfigStatus::NotFound) {
        return;
    }

    let Some(config) = report.require_parsed("norms-toml", "norms.toml", status) else {
        return;
    };

    let unexplained = config.unexplained_waivers();

    report.must(
        "norms-toml-waiver-reasons",
        "norms.toml: every waiver has a reason",
        if unexplained.is_empty() {
            Ok(())
        } else {
            Err(format!("No reason given for: {}", unexplained.join(", ")))
        },
    );

    let duplicates = config.duplicate_waivers();

    report.must(
        "norms-toml-waiver-duplicates",
        "norms.toml: no check is waived twice",
        if duplicates.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Waived more than once: {}",
                duplicates.into_iter().collect::<Vec<_>>().join(", ")
            ))
        },
    );

    let invalid = config.invalid_excludes();

    report.must(
        "norms-toml-exclude-paths",
        "norms.toml: every excluded directory is a plain relative path",
        if invalid.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Not a plain relative path: {}",
                invalid
                    .iter()
                    .map(|path| format!("{path:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        },
    );

    for path in config.excluded_paths() {
        report.should(
            "norms-toml-exclude-exists",
            format!("norms.toml: excluded directory {} exists", path.display()),
            if ctx.path().join(&path).is_dir() {
                Outcome::Pass
            } else {
                Outcome::fail("No such directory")
            },
        );
    }
}

//
// Components
//

/// Check that the search for components covered the whole project.
pub(crate) fn check_components(ctx: &ProjectContext, report: &mut Report) {
    let errors = &ctx.detection().errors;

    report.should(
        "components-search",
        "Components: every directory searched",
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        },
    );
}

//
// Required files
//

pub(crate) fn check_readme(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "readme-exists",
        "README.md",
        file_exists(&ctx.path().join("README.md")),
    );
}

pub(crate) fn check_changelog(ctx: &ProjectContext, report: &mut Report) {
    report.should(
        "changelog-exists",
        "CHANGELOG.md",
        file_exists(&ctx.path().join("CHANGELOG.md")),
    );
}

//
// Licensing
//

/// Check the license files: the REUSE texts in `LICENSES/`, their copies at the root, and `REUSE.toml`. Whether every
/// file carries SPDX headers or a `REUSE.toml` annotation is left to `reuse lint`.
pub(crate) fn check_licenses(ctx: &ProjectContext, report: &mut Report) {
    let files = ctx.license_files();

    report.must("license-exists", "LICENSE", files.has_copy());

    report.must(
        "licenses-dir-exists",
        "LICENSES/: holds the license texts",
        if files.texts.is_empty() {
            Outcome::fail("No license texts in LICENSES/")
        } else {
            Outcome::Pass
        },
    );

    if !files.texts.is_empty() || files.has_copy() {
        let problems = files.copy_problems();

        report.must(
            "license-copies",
            "LICENSE: root copies match LICENSES/",
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems.join("\n"))
            },
        );
    }

    let Some(config) = report.recommend_parsed("reuse-toml", "REUSE.toml", ctx.reuse_toml()) else {
        return;
    };

    report.must(
        "reuse-toml-version",
        "REUSE.toml: version 1",
        config.check_version(),
    );
}

//
// Git configuration
//

pub(crate) fn check_gitignore(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "gitignore-exists",
        ".gitignore",
        file_exists(&ctx.path().join(".gitignore")),
    );
}

pub(crate) fn check_gitattributes(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.gitattributes();
    let Some(content) = report.require_parsed("gitattributes", ".gitattributes", status) else {
        return;
    };

    report.should(
        "gitattributes-text-auto",
        ".gitattributes: text=auto",
        matches_regex(content, r"(?m)^\*\s.+text=auto"),
    );

    report.should(
        "gitattributes-eol-lf",
        ".gitattributes: eol=lf",
        matches_regex(content, r"(?m)^\*\s.+eol=lf"),
    );

    report.should(
        "gitattributes-binary",
        ".gitattributes: binary markers",
        matches_regex(content, r"(?m)^\*\.[a-zA-Z0-9]+\s+binary"),
    );
}

//
// Editor configuration
//

pub(crate) fn check_editorconfig(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.editorconfig();
    let Some(content) = report.require_parsed("editorconfig", ".editorconfig", status) else {
        return;
    };

    let settings = [
        ("editorconfig-root", "root=true", "root = true"),
        ("editorconfig-charset", "charset=utf-8", "charset = utf-8"),
        (
            "editorconfig-end-of-line",
            "end_of_line=lf",
            "end_of_line = lf",
        ),
        (
            "editorconfig-indent-style",
            "indent_style=space",
            "indent_style = space",
        ),
        (
            "editorconfig-indent-size",
            "indent_size=4",
            "indent_size = 4",
        ),
        (
            "editorconfig-insert-final-newline",
            "insert_final_newline=true",
            "insert_final_newline = true",
        ),
        (
            "editorconfig-trim-trailing-whitespace",
            "trim_trailing_whitespace=true",
            "trim_trailing_whitespace = true",
        ),
    ];

    for (id, label, needle) in settings {
        report.should(
            id,
            format!(".editorconfig: {label}"),
            matches_regex(content, &regex::escape(needle)),
        );
    }
}

//
// Development environment
//

/// Check `devbox.json`, which is optional: only projects that run local services use Devbox.
pub(crate) fn check_devbox(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.devbox();

    if matches!(status, ConfigStatus::NotFound) {
        return;
    }

    let Some(config) = report.require_parsed("devbox", "devbox.json", status) else {
        return;
    };

    report.should(
        "devbox-schema",
        "devbox.json: has $schema",
        config.schema.is_some(),
    );
    report.should(
        "devbox-packages",
        "devbox.json: has packages",
        !config.packages.is_empty(),
    );

    // Check for devbox.lock
    report.should(
        "devbox-lock-exists",
        "devbox.lock",
        file_exists(&ctx.path().join("devbox.lock")),
    );
}

//
// Pre-commit
//

pub(crate) fn check_precommit(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "precommit-config-exists",
        ".pre-commit-config.yaml",
        file_exists(&ctx.path().join(".pre-commit-config.yaml")),
    );

    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.should(
        "precommit-reuse",
        "Pre-commit: reuse hook",
        config.has_hook("reuse"),
    );
    report.should(
        "precommit-tombi",
        "Pre-commit: tombi-format hook",
        config.has_hook("tombi-format"),
    );
    report.should(
        "precommit-zizmor",
        "Pre-commit: zizmor hook (pedantic)",
        if !config.has_hook("zizmor") {
            Err("No zizmor hook".to_owned())
        } else if config.hooks("zizmor").any(zizmor_hook_strict) {
            Ok(())
        } else {
            Err("The zizmor hook doesn't pass --pedantic".to_owned())
        },
    );
}

//
// zizmor
//

/// The zizmor personas that report at least what the pedantic one does: pedantic itself, and auditor.
const STRICT_ZIZMOR_PERSONAS: [&str; 2] = ["pedantic", "auditor"];

/// Whether zizmor's command-line arguments select a persona at least as strict as pedantic: `--pedantic` (or `-p`), or
/// `--persona` naming one, as `--persona=<name>` or `--persona <name>`.
fn zizmor_args_strict<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let persona = match arg {
            "--pedantic" | "-p" => return true,
            "--persona" => args.next(),
            _ => arg.strip_prefix("--persona="),
        };
        if persona.is_some_and(|persona| STRICT_ZIZMOR_PERSONAS.contains(&persona)) {
            return true;
        }
    }
    false
}

/// Whether a zizmor hook runs a persona at least as strict as pedantic, from its args or, for a local hook, its entry,
/// to which pre-commit appends the args.
fn zizmor_hook_strict(hook: &PrecommitHook) -> bool {
    let entry = hook.entry.as_deref().unwrap_or_default().split_whitespace();
    zizmor_args_strict(
        entry
            .skip_while(|word| !is_zizmor_command(word))
            .skip(1)
            .chain(hook.args.iter().map(String::as_str)),
    )
}

/// Whether a word of a shell command invokes zizmor: `zizmor`, a path to it, or a versioned `zizmor@x.y.z` as uvx
/// takes it.
fn is_zizmor_command(word: &str) -> bool {
    let name = word.rsplit('/').next().unwrap_or(word);
    name == "zizmor" || name.starts_with("zizmor@") || name.starts_with("zizmor==")
}

/// Whether `workflow` runs zizmor so that its findings fail the job, with a persona at least as strict as pedantic:
/// either a `run` script line invoking it with such a persona, or a `zizmorcore/zizmor-action` step setting one in
/// `persona` and turning off `advanced-security`, whose default uploads findings to code scanning and passes the job.
fn zizmor_in_ci(workflow: &Workflow) -> Result<(), String> {
    let mut issues = Vec::new();

    for line in workflow.run_lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(start) = words.iter().position(|word| is_zizmor_command(word)) else {
            continue;
        };
        if zizmor_args_strict(words.iter().skip(start + 1).copied()) {
            return Ok(());
        }
        issues.push("A step runs zizmor without --pedantic".to_owned());
    }

    for step in workflow.steps_using("zizmorcore/zizmor-action") {
        let strict = step
            .input("persona")
            .is_some_and(|persona| STRICT_ZIZMOR_PERSONAS.contains(&persona.as_ref()));
        let gating = step.input("advanced-security").as_deref() == Some("false");
        match (strict, gating) {
            (true, true) => return Ok(()),
            (false, true) => issues.push("zizmor-action doesn't set persona: pedantic".to_owned()),
            (true, false) => {
                issues.push("zizmor-action doesn't set advanced-security: false".to_owned());
            }
            (false, false) => issues.push(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false"
                    .to_owned(),
            ),
        }
    }

    if issues.is_empty() {
        Err("No step runs zizmor or uses zizmorcore/zizmor-action".to_owned())
    } else {
        issues.dedup();
        Err(issues.join("; "))
    }
}

//
// Dependency management
//

pub(crate) fn check_renovate(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.recommend_parsed("renovate", "renovate.json", ctx.renovate()) else {
        return;
    };

    report.should(
        "renovate-schema",
        "renovate.json: has $schema",
        config.schema.is_some(),
    );
    report.should(
        "renovate-extends-preset",
        "renovate.json: extends the shared preset",
        if config.extends_shared_preset() {
            Ok(())
        } else {
            Err(format!("Doesn't extend {SHARED_PRESET}"))
        },
    );
}

//
// Continuous integration
//

/// `reuse lint` in a CI script, but not `reuse lint-file`, which lints only the files it's given.
static REUSE_LINT: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)\breuse\s+lint(?:$|[^\w-])").unwrap()
});

/// `tombi format --check` in a CI script, which fails on unformatted files instead of rewriting them.
static TOMBI_FORMAT_CHECK: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)\btombi\s+format\b[^\n]*\s--check\b").unwrap()
});

pub(crate) fn check_github_actions(ctx: &ProjectContext, report: &mut Report) {
    let workflows_dir = ctx.path().join(".github/workflows");
    report.must(
        "ci-workflows-dir-exists",
        "CI: .github/workflows/ exists",
        file_exists(&workflows_dir),
    );

    if !workflows_dir.exists() {
        return;
    }

    report.require_parsed("ci-yaml", "CI: ci.yaml", ctx.ci_workflow());

    if let Some(audit) =
        report.recommend_parsed("audit-yaml", "CI: audit.yaml", ctx.audit_workflow())
    {
        report.should(
            "audit-yaml-schedule",
            "CI: audit.yaml: runs on a schedule",
            audit.runs_on_schedule(),
        );
    }

    // Every other workflow must parse too. The checks above already cover ci.yaml and audit.yaml.
    for (name, status) in ctx
        .workflows()
        .iter()
        .filter(|(name, _)| *name != CI_WORKFLOW && *name != AUDIT_WORKFLOW)
    {
        report.must(
            "workflow-valid",
            format!("CI: {name}: valid"),
            match status {
                ConfigStatus::NotFound => Outcome::fail("File not found"),
                ConfigStatus::ParseError { error, .. } => Outcome::fail(error.clone()),
                ConfigStatus::Ok(_) => Outcome::Pass,
            },
        );
    }

    for (name, workflow) in ctx
        .workflows()
        .iter()
        .filter_map(|(name, status)| Some((name, status.as_ref_ok()?)))
    {
        let untimed = workflow.jobs_without_timeout();
        report.should(
            "workflow-job-timeouts",
            format!("CI: {name}: every job sets timeout-minutes"),
            if untimed.is_empty() {
                Ok(())
            } else {
                Err(format!("No timeout-minutes: {}", untimed.join(", ")))
            },
        );
        report.should(
            "workflow-concurrency",
            format!("CI: {name}: concurrency cancels superseded runs"),
            workflow.cancels_superseded_runs(),
        );
    }

    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    report.should(
        "ci-reuse",
        "CI: REUSE compliance",
        workflow.uses_action("fsfe/reuse-action") || workflow.runs_matching(&REUSE_LINT),
    );
    report.should(
        "ci-tombi",
        "CI: TOML formatting",
        workflow.runs_matching(&TOMBI_FORMAT_CHECK),
    );
    report.should("ci-zizmor", "CI: zizmor (pedantic)", zizmor_in_ci(workflow));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_lint_excludes_lint_file() {
        for run in [
            "reuse lint",
            "uvx reuse lint --json",
            "reuse  lint\necho done",
            "reuse lint; echo",
        ] {
            assert!(REUSE_LINT.is_match(run), "{run}");
        }
        for run in [
            "reuse lint-file src/main.rs",
            "reuse --root . lint",
            "nonreuse lint",
        ] {
            assert!(!REUSE_LINT.is_match(run), "{run}");
        }
    }

    #[test]
    fn zizmor_args_need_a_strict_persona() {
        for args in [
            "--pedantic",
            "--offline --pedantic",
            "-p",
            "--persona=pedantic",
            "--persona=auditor",
            "--persona pedantic .",
        ] {
            assert!(zizmor_args_strict(args.split_whitespace()), "{args}");
        }
        for args in [
            "",
            "--offline",
            "--persona=regular",
            "--persona",
            "--persona regular --pedantic-ish",
            "pedantic",
        ] {
            assert!(!zizmor_args_strict(args.split_whitespace()), "{args}");
        }
    }

    #[test]
    fn zizmor_hooks_need_a_strict_persona() {
        let hook = |entry: Option<&str>, args: &[&str]| PrecommitHook {
            id: "zizmor".to_owned(),
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrecommitHook::default()
        };
        assert!(zizmor_hook_strict(&hook(None, &["--pedantic"])));
        assert!(zizmor_hook_strict(&hook(Some("zizmor --pedantic"), &[])));
        assert!(zizmor_hook_strict(&hook(
            Some("uvx zizmor"),
            &["--persona", "auditor"]
        )));
        assert!(!zizmor_hook_strict(&hook(None, &[])));
        assert!(!zizmor_hook_strict(&hook(Some("zizmor --offline"), &["."])));
        assert!(!zizmor_hook_strict(&hook(Some("echo --pedantic"), &[])));
    }

    fn zizmor_in(yaml: &str) -> Result<(), String> {
        let workflow: Workflow = serde_saphyr::from_str(yaml).expect("workflow parses");
        zizmor_in_ci(&workflow)
    }

    #[test]
    fn zizmor_runs_in_ci() {
        let script = |run: &str| format!("jobs:\n  z:\n    steps:\n      - run: \"{run}\"\n");
        for run in [
            "zizmor --pedantic .",
            "uvx zizmor@1.32.0 --persona=pedantic --format github .",
            "echo start\\n  ~/.cargo/bin/zizmor --pedantic .",
            "zizmor --format github \\\\\\n  --pedantic .",
        ] {
            assert_eq!(zizmor_in(&script(run)), Ok(()), "{run}");
        }
        assert_eq!(
            zizmor_in(&script("zizmor .")),
            Err("A step runs zizmor without --pedantic".to_owned())
        );
        assert_eq!(
            zizmor_in(&script("echo zizmorish --pedantic")),
            Err("No step runs zizmor or uses zizmorcore/zizmor-action".to_owned())
        );
    }

    #[test]
    fn zizmor_action_must_be_pedantic_and_gating() {
        let step = |with: &str| {
            format!(
                "jobs:\n  z:\n    steps:\n      - uses: zizmorcore/zizmor-action@abc # v0.8.1\n        with: {{{with}}}\n"
            )
        };
        assert_eq!(
            zizmor_in(&step("persona: pedantic, advanced-security: false")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("persona: auditor, advanced-security: \"false\"")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("advanced-security: false")),
            Err("zizmor-action doesn't set persona: pedantic".to_owned())
        );
        assert_eq!(
            zizmor_in(&step("persona: pedantic")),
            Err("zizmor-action doesn't set advanced-security: false".to_owned())
        );
        assert_eq!(
            zizmor_in(&step("")),
            Err(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false"
                    .to_owned()
            )
        );
    }

    #[test]
    fn tombi_format_must_check() {
        for run in [
            "tombi format --check",
            "tombi format --check --diagnostics-format github",
            "uvx tombi format . --check",
        ] {
            assert!(TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
        for run in [
            "tombi format",
            "tombi format\necho --check",
            "tombi lint --check",
            "tombi format --checked",
        ] {
            assert!(!TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
    }
}

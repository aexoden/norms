// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects reqgardless of language.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{DevboxConfig, PrecommitHook, SHARED_PRESET, Workflow, commands};
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

    report.should(
        "devbox-no-toolchains",
        "devbox.json: installs no language toolchains",
        devbox_toolchains(config),
    );

    // Check for devbox.lock
    report.should(
        "devbox-lock-exists",
        "devbox.lock",
        file_exists(&ctx.path().join("devbox.lock")),
    );
}

/// A Nix package or flake name that is a toolchain, including versioned attributes such as `nodejs_22`, or its
/// version or package manager, for a language norms checks: Rust (rustup, rustc, cargo, clippy, rustfmt, and the
/// rust-overlay and fenix flakes), Python (`python*`, `pypy*`, uv, pip, pipx, Poetry, PDM, Hatch, Rye, pyenv, Conda) or
/// TypeScript (`nodejs*`, pnpm, Yarn, npm, Corepack, Bun, Deno).
static DEVBOX_TOOLCHAIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(concat!(
        r"^(?:rustup|rustc|cargo|clippy|rustfmt|rust-bin|rust-overlay|fenix",
        r"|python3?[0-9]*(?:-?full|-?minimal)?|pypy[0-9]*|uv|pip|pipx|poetry|pdm|hatch|rye|pyenv|conda|micromamba",
        r"|nodejs(?:-slim)?(?:_[0-9]+|_latest|-[0-9]+_x)?|pnpm(?:_[0-9]+|_latest)?|yarn(?:-berry(?:_[0-9]+)?)?|npm",
        r"|corepack(?:_[0-9]+|_latest)?|bun|deno)$",
    ))
    .unwrap()
});

/// The names under which a Devbox package reference may name a toolchain, lowercased so that mixed-case attributes
/// such as `python313Full` match: the first and last parts of its attribute path (`rust-bin.stable.latest.default`,
/// `python312Packages.pip`), taken after a flake reference's `#` and without any `@version`, plus the repository of a
/// `github:` flake (`github:nix-community/fenix#stable.toolchain`, `github:astral-sh/uv`) or a `runx:` package
/// (`runx:astral-sh/uv`).
fn devbox_package_names(package: &str) -> Vec<String> {
    let mut names = Vec::new();

    if let Some(repo) = package.strip_prefix("runx:") {
        let repo = repo.split_once('@').map_or(repo, |(repo, _)| repo);
        names.extend(repo.rsplit('/').next());
    } else {
        // A flake reference without an attribute (`github:astral-sh/uv`) installs the flake's default package.
        let (flake, attr) = match package.split_once('#') {
            Some((flake, attr)) => (Some(flake), Some(attr)),
            None if package.contains(':') => (Some(package), None),
            None => (None, Some(package)),
        };
        if let Some(path) = flake.and_then(|flake| flake.strip_prefix("github:")) {
            names.extend(path.split('/').nth(1));
        }
        if let Some(attr) = attr {
            let attr = attr.split_once('@').map_or(attr, |(attr, _)| attr);
            names.extend(attr.split('.').next());
            names.extend(attr.rsplit('.').next());
        }
    }

    names.iter().map(|name| name.to_ascii_lowercase()).collect()
}

/// Whether `devbox.json` installs no language toolchains, as packages or by enabling Corepack, which Devbox's
/// Node.js plugin does when `DEVBOX_COREPACK_ENABLED` is set to anything.
fn devbox_toolchains(config: &DevboxConfig) -> Result<(), String> {
    let mut problems = Vec::new();

    let toolchains: Vec<&str> = config
        .packages
        .iter()
        .filter(|package| {
            devbox_package_names(package)
                .iter()
                .any(|name| DEVBOX_TOOLCHAIN.is_match(name))
        })
        .map(String::as_str)
        .collect();
    if !toolchains.is_empty() {
        problems.push(format!("installs {}", toolchains.join(", ")));
    }

    if config
        .env
        .as_ref()
        .and_then(|env| env.get("DEVBOX_COREPACK_ENABLED"))
        .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    {
        problems.push("enables Corepack (DEVBOX_COREPACK_ENABLED)".to_owned());
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "devbox.json {}; install toolchains and their managers outside Devbox",
            problems.join(" and ")
        ))
    }
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
        "precommit-install-hook-types",
        "Pre-commit: default_install_hook_types includes pre-commit and commit-msg",
        config.check_default_install_hook_types(),
    );
    report.should(
        "precommit-default-stages",
        "Pre-commit: default_stages is [pre-commit]",
        config.check_default_stages(),
    );
    report.should(
        "precommit-hook-ids-unique",
        "Pre-commit: each hook has a unique ID or alias",
        config.check_unique_hook_ids(),
    );
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
// pre-commit in CI
//

/// A word of a shell command, with any quotes around it removed, that pins pre-commit to a version: as uvx takes it
/// (`pre-commit@x.y.z`), or as pip and `uvx --from` do (`pre-commit==x.y.z`, also after `--from=`). A version from a
/// shell variable counts, since its value can't be seen here.
static PRE_COMMIT_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?:^|[=/])pre-commit(?:@|==)(?:[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc|\.post|\.dev)[0-9]+)?$|\$)").unwrap()
});

/// Whether a word of a shell command pins pre-commit to a version.
fn is_pre_commit_pin(word: &str) -> bool {
    PRE_COMMIT_PIN.is_match(word.trim_matches(['"', '\'']))
}

/// Whether a word of a shell command, with any quotes around it removed, invokes pre-commit: `pre-commit`, a path to
/// it, or a versioned `pre-commit@x.y.z` as uvx takes it.
fn is_pre_commit_command(word: &str) -> bool {
    let word = word.trim_matches(['"', '\'']);
    let name = word.rsplit('/').next().unwrap_or(word);
    name == "pre-commit" || name.starts_with("pre-commit@")
}

/// Whether `workflow` runs every pre-commit hook it doesn't skip: a script runs `pre-commit run` with `--all-files`
/// (or `-a`), with a pinned pre-commit. The pin belongs to that command, as `uvx pre-commit@x.y.z` or `uvx --from
/// pre-commit==x.y.z pre-commit`, or, for a plain `pre-commit run`, to an earlier command of the same job that
/// installs it, such as `pip install pre-commit==x.y.z`.
fn pre_commit_in_ci(workflow: &Workflow) -> Result<(), String> {
    let mut runs = false;
    let mut all_files = false;
    let mut pinned = false;

    for job in workflow.jobs.values() {
        let mut installed = false;
        for line in job.run_lines() {
            for words in commands(&line) {
                let Some(start) = words.windows(2).position(
                    |pair| matches!(pair, [program, "run"] if is_pre_commit_command(program)),
                ) else {
                    installed |= words.iter().any(|word| is_pre_commit_pin(word));
                    continue;
                };
                runs = true;
                if !words
                    .iter()
                    .skip(start + 2)
                    .any(|arg| matches!(*arg, "--all-files" | "-a"))
                {
                    continue;
                }
                all_files = true;
                pinned |= words
                    .iter()
                    .take(start + 1)
                    .any(|word| is_pre_commit_pin(word))
                    || (start == 0 && installed);
            }
        }
    }

    if !runs {
        Err(if workflow.uses_action("pre-commit/action") {
            "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned".to_owned()
        } else {
            "No step runs pre-commit run".to_owned()
        })
    } else if !all_files {
        Err("pre-commit run doesn't pass --all-files".to_owned())
    } else if !pinned {
        Err(
            "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)"
                .to_owned(),
        )
    } else {
        Ok(())
    }
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
    report.should(
        "ci-pre-commit",
        "CI: remaining pre-commit hooks (pinned, --all-files)",
        pre_commit_in_ci(workflow),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pre_commit(run: &str) -> Result<(), String> {
        let yaml = format!("jobs:\n  hooks:\n    steps:\n      - run: {run:?}\n");
        pre_commit_in_ci(&serde_saphyr::from_str(&yaml).unwrap())
    }

    #[test]
    fn pre_commit_runs_every_file_pinned() {
        for run in [
            "uvx pre-commit@4.3.0 run --all-files --show-diff-on-failure",
            "uvx \"pre-commit@$PRE_COMMIT_VERSION\" run --all-files",
            "uvx --from pre-commit==4.3.0 pre-commit run -a",
            "pip install pre-commit==4.3.0\npre-commit run --show-diff-on-failure --all-files",
            "uv tool install 'pre-commit==4.3.0' && pre-commit run \\\n  --all-files",
            "uvx --from=pre-commit==4.3.0rc1 pre-commit run -a",
        ] {
            assert_eq!(pre_commit(run), Ok(()), "{run}");
        }
        for (run, error) in [
            ("pre-commit install", "No step runs pre-commit run"),
            (
                "uvx pre-commit@4.3.0 run",
                "pre-commit run doesn't pass --all-files",
            ),
            (
                "uvx pre-commit@4.3.0 run --all-files-later",
                "pre-commit run doesn't pass --all-files",
            ),
            (
                "uvx pre-commit@4.3.0 run cargo-test; echo --all-files",
                "pre-commit run doesn't pass --all-files",
            ),
            (
                "uvx pre-commit run --all-files",
                "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
            ),
            (
                "uvx pre-commit@latest run --all-files",
                "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
            ),
            (
                "pip install pre-commit==4.3.0\nuvx pre-commit run -a",
                "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
            ),
            (
                "pip install pre-commit>=4 && pre-commit run -a",
                "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
            ),
        ] {
            assert_eq!(pre_commit(run), Err(error.to_owned()), "{run}");
        }

        // A pin in another job doesn't apply.
        let split: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - run: pip install pre-commit==4.3.0\n  b:\n    steps:\n      - run: pre-commit run -a\n",
        )
        .unwrap();
        assert!(pre_commit_in_ci(&split).is_err());
        let action: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - uses: pre-commit/action@v3.0.1\n",
        )
        .unwrap();
        assert_eq!(
            pre_commit_in_ci(&action),
            Err(
                "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned"
                    .to_owned()
            )
        );
    }

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

    fn devbox(json: &str) -> Result<(), String> {
        devbox_toolchains(&json5::from_str(json).unwrap())
    }

    fn is_devbox_toolchain(package: &str) -> bool {
        devbox_package_names(package)
            .iter()
            .any(|name| DEVBOX_TOOLCHAIN.is_match(name))
    }

    #[test]
    fn devbox_package_names_parsed() {
        for (package, names) in [
            ("postgresql", vec!["postgresql", "postgresql"]),
            ("nodejs@22.11.0", vec!["nodejs", "nodejs"]),
            (
                "github:NixOS/nixpkgs/5233fd2#nodejs_22",
                vec!["nixpkgs", "nodejs_22", "nodejs_22"],
            ),
            ("nixpkgs#uv@latest", vec!["uv", "uv"]),
            ("python312Packages.pip", vec!["python312packages", "pip"]),
            ("runx:astral-sh/uv@0.9.0", vec!["uv"]),
            ("runx:astral-sh/uv", vec!["uv"]),
            ("python3Full", vec!["python3full", "python3full"]),
            ("github:astral-sh/uv", vec!["uv"]),
            ("github:NixOS/nixpkgs/nixos-24.05", vec!["nixpkgs"]),
            ("path:./flakes/db", vec![]),
            (
                "github:oxalica/rust-overlay#rust-bin.stable.latest.default",
                vec!["rust-overlay", "rust-bin", "default"],
            ),
        ] {
            assert_eq!(devbox_package_names(package), names, "{package}");
        }
    }

    #[test]
    fn devbox_toolchain_packages() {
        for package in [
            "rustup",
            "rustc@1.91",
            "cargo",
            "clippy",
            "rustfmt",
            "github:oxalica/rust-overlay#rust-bin.stable.latest.default",
            "github:nix-community/fenix#stable.toolchain",
            "python3",
            "python312",
            "python313Full",
            "python3Minimal",
            "pypy3",
            "uv@0.9",
            "pip",
            "pipx",
            "poetry",
            "pdm",
            "hatch",
            "rye",
            "pyenv",
            "conda",
            "micromamba",
            "nodejs",
            "nodejs@24",
            "nodejs_22",
            "nodejs_latest",
            "nodejs-slim",
            "nodejs-slim_24",
            "nodejs-slim_latest",
            "nodejs-18_x",
            "pnpm@12",
            "pnpm_10",
            "pnpm_latest",
            "yarn",
            "yarn-berry",
            "yarn-berry_4",
            "npm",
            "corepack",
            "corepack_22",
            "corepack_latest",
            "bun",
            "deno",
            "nixpkgs#pnpm",
            "python312Packages.poetry",
            "github:astral-sh/uv",
            "runx:astral-sh/uv",
        ] {
            assert!(is_devbox_toolchain(package), "{package}");
        }
        for package in [
            "postgresql@17",
            "postgresql_16",
            "sqlx-cli@0.8",
            "cargo-deny",
            "cargo-watch",
            "pre-commit",
            "redis",
            "python312Packages.psycopg",
            "nodePackages.prettier",
            "rust-analyzer",
            "uvicorn",
            "deno-lint",
            "github:NixOS/nixpkgs",
            "github:NixOS/nixpkgs/nixos-24.05#postgresql",
            "path:./flakes/db#db",
        ] {
            assert!(!is_devbox_toolchain(package), "{package}");
        }
    }

    #[test]
    fn devbox_toolchains_reported() {
        assert_eq!(
            devbox(r#"{"packages": ["postgresql@17", "sqlx-cli@0.8"]}"#),
            Ok(())
        );
        assert_eq!(
            devbox(r#"{"packages": {"redis": "latest"}, "env": {"DEVBOX_COREPACK_ENABLED": ""}}"#),
            Ok(())
        );
        assert_eq!(devbox(r#"{"packages": ["redis"], "env": null}"#), Ok(()));
        assert_eq!(
            devbox(
                r#"{"packages": ["redis"], "env": {"PORT": 6379, "DEVBOX_COREPACK_ENABLED": null}}"#
            ),
            Ok(())
        );
        assert_eq!(
            devbox(r#"{"packages": ["postgresql@17", "nodejs@24", "pnpm@11.22.0"]}"#),
            Err("devbox.json installs nodejs@24, pnpm@11.22.0; install toolchains and their managers outside Devbox".to_owned())
        );
        assert_eq!(
            devbox(r#"{"packages": {"uv": {"version": "latest"}}, "env": {"DEVBOX_COREPACK_ENABLED": "true"}}"#),
            Err(
                "devbox.json installs uv and enables Corepack (DEVBOX_COREPACK_ENABLED); install toolchains and their managers outside Devbox"
                    .to_owned()
            )
        );
    }
}

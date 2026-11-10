// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Python-specific checks.

use pep508_rs::pep440_rs::Version;

use crate::checks::common::{SETUP_UV, is_exact_uv_version};
use crate::config::{
    FormatterRun, GIT_ARCHIVAL, PYTHON_VERSION, PrecommitHook, PyprojectToml, PytestConfig,
    Workflow, check_python_version_exact, check_release_age_value, commands, program_args,
    sets_attribute,
};
use crate::context::{ComponentContext, ConfigStatus, ProjectContext};
use crate::report::{Issue, Outcome, Report, file_exists};

/// Whether the component is a published library: its `pyproject.toml` parses and describes a package that installs no
/// command and isn't marked private.
pub(crate) fn publishes_library(ctx: &ComponentContext) -> bool {
    ctx.pyproject()
        .as_ref_ok()
        .is_some_and(|config| config.is_library() && config.is_publishable())
}

//
// Project configuration
//

pub(crate) fn check_pyproject(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("pyproject", "pyproject.toml", ctx.pyproject()) else {
        return;
    };

    report.must(
        "pyproject-project",
        "pyproject.toml: has [project] section",
        config.project.is_some(),
    );

    if let Some(license) = config.project.as_ref().and_then(|p| p.license.as_ref()) {
        report.check(
            "pyproject-license-texts",
            "pyproject.toml: license has its texts in LICENSES/",
            license.as_str().map_or_else(
                || Err(Issue::warn("Not an SPDX expression string")),
                |expression| ctx.license_files().check_expression(expression),
            ),
        );
    }
}

/// Check that the component pins its development Python with an exact version in its own `.python-version`.
pub(crate) fn check_python_version(ctx: &ComponentContext, report: &mut Report) {
    report.must(
        "python-version-exists",
        "Python: .python-version",
        file_exists(&ctx.path().join(PYTHON_VERSION)),
    );
    // A pin that isn't a plain version, such as `cpython@3.15`, is reported here rather than as invalid: uv accepts
    // it, but it isn't the exact version the standards ask for.
    let exact = match ctx.python_version() {
        ConfigStatus::NotFound => return,
        ConfigStatus::ParseError { error } => Err(error.clone()),
        ConfigStatus::Ok(version) => check_python_version_exact(version),
    };
    report.should(
        "python-version-exact",
        "Python: .python-version pins an exact version",
        exact,
    );
}

/// Check that `requires-python` agrees with the version in `.python-version`.
pub(crate) fn check_requires_python_pin(ctx: &ComponentContext, report: &mut Report) {
    // python-version-exists and python-version-exact report a missing or inexact pin.
    let (Some(config), ConfigStatus::Ok(pinned)) =
        (ctx.pyproject().as_ref_ok(), ctx.python_version())
    else {
        return;
    };
    if check_python_version_exact(pinned).is_err() {
        return;
    }

    report.should(
        "pyproject-requires-python-pin",
        "pyproject.toml: requires-python agrees with .python-version",
        config.check_requires_python_pin(pinned),
    );
}

/// Check that the Python classifiers match `requires-python`, up to the version in `.python-version`.
pub(crate) fn check_python_classifiers(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    let pinned = match ctx.python_version() {
        ConfigStatus::Ok(version) => Some(version),
        ConfigStatus::NotFound => None,
        // python-version-exact reports it.
        ConfigStatus::ParseError { .. } => return,
    };

    report.should(
        "pyproject-python-classifiers",
        "pyproject.toml: Python classifiers match requires-python",
        config.check_python_classifiers(pinned),
    );
}

/// Check that a packaged component takes its version from git tags through hatch-vcs, and that uv reinstalls it when
/// they move. A component without a `[project]` table, or with `[tool.uv] package = false`, has no version to take.
pub(crate) fn check_versioning(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    if !config.has_version() {
        return;
    }

    report.should(
        "pyproject-version-dynamic",
        "pyproject.toml: version is dynamic",
        config.check_version_dynamic(),
    );
    report.check(
        "pyproject-build-system",
        "pyproject.toml: builds with hatchling and hatch-vcs",
        config.check_build_system(),
    );
    report.check(
        "pyproject-hatch-version",
        "pyproject.toml: version from git tags (hatch-vcs)",
        config.check_hatch_version(!ctx.component().path.as_os_str().is_empty()),
    );
    report.should(
        "uv-cache-keys",
        "uv: reinstalls the project when pyproject.toml, HEAD or tags change (cache-keys)",
        config.check_uv_cache_keys(),
    );
}

/// The keys of `.git_archival.txt` that hatch-vcs reads the version from, each with the placeholder git fills in: the
/// commit, and the description from the nearest tag, lightweight tags included.
const GIT_ARCHIVAL_KEYS: [(&str, &str); 2] = [
    ("node", "$Format:%H$"),
    (
        "describe-name",
        "$Format:%(describe:tags=true,match=*[0-9]*)$",
    ),
];

/// Check `.git_archival.txt`'s `node` and `describe-name` lines. An older form with only `ref-names` reports a version
/// only when the archived commit is itself tagged. A value that is a placeholder must be the standard one, but one
/// that isn't was filled in by `git archive`, as in an extracted source archive, so it can't be checked.
fn check_git_archival_content(content: &str) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut other = Vec::new();
    for (key, placeholder) in GIT_ARCHIVAL_KEYS {
        let value = content.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
                .map(str::trim)
        });
        match value {
            None => missing.push(key),
            Some(value) if value.starts_with("$Format:") && value != placeholder => {
                other.push(format!("{key} is {value}, not {placeholder}"));
            }
            Some(_) => {}
        }
    }
    if !missing.is_empty() {
        other.insert(
            0,
            format!("{GIT_ARCHIVAL} has no {} line", missing.join(" or ")),
        );
    }
    if other.is_empty() {
        Ok(())
    } else {
        Err(other.join("; "))
    }
}

/// Check that a packaged component has a `.git_archival.txt`, so that source archives report their version, and that
/// the repository's `.gitattributes` marks it `export-subst`, so that git fills it in.
pub(crate) fn check_git_archival(
    project: &ProjectContext,
    ctx: &ComponentContext,
    report: &mut Report,
) {
    if !ctx
        .pyproject()
        .as_ref_ok()
        .is_some_and(PyprojectToml::has_version)
    {
        return;
    }

    report.check(
        "python-git-archival",
        "Python: .git_archival.txt (node and describe-name)",
        match ctx.git_archival() {
            ConfigStatus::Ok(content) => check_git_archival_content(content).map_err(Issue::warn),
            ConfigStatus::NotFound => Err(Issue::fail(format!("Not found: {GIT_ARCHIVAL}"))),
            ConfigStatus::ParseError { error } => Err(Issue::fail(error.clone())),
        },
    );

    // gitattributes-exists reports a missing or unreadable file, and python-git-archival a missing archival file. Only
    // the root .gitattributes is read, where the standards put the attribute.
    if let (ConfigStatus::Ok(gitattributes), ConfigStatus::Ok(_)) =
        (project.gitattributes(), ctx.git_archival())
    {
        let path = ctx.path().join(GIT_ARCHIVAL);
        report.should(
            "gitattributes-git-archival",
            ".gitattributes: .git_archival.txt marked export-subst",
            if sets_attribute(gitattributes, project.path(), &path, "export-subst") {
                Ok(())
            } else {
                Err(format!(
                    "No line marks {} export-subst",
                    ctx.component().path.join(GIT_ARCHIVAL).display()
                ))
            },
        );
    }
}

/// Check that `[tool.uv] required-version` sets the oldest uv the component needs, and that every exact uv version a
/// workflow's setup-uv step pins satisfies it. workflow-setup-uv-version reports the steps pinning no exact version.
pub(crate) fn check_uv_required_version(
    project: &ProjectContext,
    ctx: &ComponentContext,
    report: &mut Report,
) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    if uses_uv_toml(ctx) {
        return;
    }

    report.should(
        "uv-required-version",
        "uv: required-version sets the oldest uv needed (>=)",
        config.check_uv_required_version(),
    );

    let Ok(required) = config.uv_required_version() else {
        return;
    };
    let mut pinned = false;
    let mut rejected = Vec::new();
    for (name, workflow) in project
        .workflows()
        .iter()
        .filter_map(|(name, status)| Some((name, status.as_ref_ok()?)))
    {
        for (job, step) in workflow.job_steps_using(SETUP_UV) {
            let Some(version) = step.input("version") else {
                continue;
            };
            let Some(parsed) = is_exact_uv_version(&version)
                .then(|| version.parse::<Version>().ok())
                .flatten()
            else {
                continue;
            };
            pinned = true;
            if !required.contains(&parsed) {
                rejected.push(format!("{name}: {job} ({version})"));
            }
        }
    }
    if pinned {
        report.must(
            "uv-required-version-ci",
            "uv: CI's pinned uv satisfies required-version",
            if rejected.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "required-version ({required}) rejects the uv pinned in {}",
                    rejected.join(", ")
                ))
            },
        );
    }
}

//
// Project layout
//

pub(crate) fn check_src_layout(ctx: &ComponentContext, report: &mut Report) {
    let src_path = ctx.path().join("src");

    if !src_path.is_dir() {
        report.should(
            "python-src-layout",
            "Python: src/ directory",
            Outcome::issue("No src/ directory"),
        );

        return;
    }
    report.should("python-src-layout", "Python: src/ directory", Outcome::Pass);

    let has_packages = src_path
        .read_dir()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.path().is_dir() && entry.path().join("__init__.py").exists());

    report.should(
        "python-src-packages",
        "Python: src/ contains packages",
        if has_packages {
            Outcome::Pass
        } else {
            Outcome::issue("src/ holds no package, a directory with an __init__.py")
        },
    );
}

//
// Dependency management
//

pub(crate) fn check_uv_lock(ctx: &ComponentContext, report: &mut Report) {
    report.must(
        "uv-lock-exists",
        "Python: uv.lock",
        file_exists(&ctx.path().join("uv.lock")),
    );
}

pub(crate) fn check_dependencies_exact(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    if config.is_application() {
        report.should(
            "pyproject-dependencies-exact",
            "pyproject.toml: dependencies pinned exactly (application)",
            config.check_dependencies_exact(),
        );
    }
}

pub(crate) fn check_uv_exclude_newer(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    if uses_uv_toml(ctx) {
        return;
    }

    let exclude_newer = config
        .tool
        .as_ref()
        .and_then(|t| t.uv.as_ref())
        .and_then(|uv| uv.exclude_newer.as_ref());

    report.should(
        "uv-exclude-newer",
        "uv: release age of 3 days (exclude-newer)",
        exclude_newer.map_or_else(
            || Err("Missing [tool.uv] exclude-newer".to_owned()),
            check_release_age_value,
        ),
    );
}

pub(crate) fn check_dependency_groups(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    if let Some(groups) = &config.dependency_groups {
        report.should(
            "python-dependency-groups",
            "Python: has dependency-groups",
            Outcome::Pass,
        );
        report.should(
            "python-dev-group",
            "Python: has dev dependency group",
            groups.contains_key("dev"),
        );
    } else {
        report.should(
            "python-dependency-groups",
            "Python: has dependency-groups",
            Outcome::issue("Missing [dependency-groups]"),
        );
    }
}

//
// Ruff
//

/// The line length the standards set for Ruff, as for Prettier's `printWidth`.
const RUFF_LINE_LENGTH: u32 = 120;

/// Check that `length`, Ruff's `line-length`, is [`RUFF_LINE_LENGTH`].
fn check_line_length(length: Option<u32>) -> Result<(), String> {
    match length {
        Some(RUFF_LINE_LENGTH) => Ok(()),
        Some(length) => Err(format!("line-length is {length}, not {RUFF_LINE_LENGTH}")),
        None => Err("Missing line-length".to_owned()),
    }
}

/// The configuration files Ruff prefers to `[tool.ruff]` in a `pyproject.toml` beside them, in its order.
const RUFF_CONFIGS: [&str; 2] = [".ruff.toml", "ruff.toml"];

/// The configuration files mypy prefers to `[tool.mypy]` in a `pyproject.toml` beside them, in its order.
const MYPY_CONFIGS: [&str; 2] = ["mypy.ini", ".mypy.ini"];

/// uv's configuration file, which takes the place of `[tool.uv]` in a `pyproject.toml` beside it.
const UV_TOML: &str = "uv.toml";

/// Check that uv reads its settings from `[tool.uv]`: a `uv.toml` beside `pyproject.toml` takes the place of its
/// settings, such as `exclude-newer` and `required-version`, so the checks of those are left out. uv still reads the
/// project's own fields from `[tool.uv]`, such as `cache-keys` and `package`.
pub(crate) fn check_uv_config(ctx: &ComponentContext, report: &mut Report) {
    report.should(
        "uv-config",
        "uv: settings in [tool.uv], without a uv.toml",
        if uses_uv_toml(ctx) {
            Err(format!(
                "uv uses {UV_TOML}, which takes the place of the settings in [tool.uv] in pyproject.toml"
            ))
        } else {
            Ok(())
        },
    );
}

/// Whether uv reads its settings from a `uv.toml` in the component, rather than from `[tool.uv]`.
fn uses_uv_toml(ctx: &ComponentContext) -> bool {
    ctx.first_existing(&[UV_TOML]).is_some()
}

pub(crate) fn check_ruff(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    if let Some(name) = ctx.first_existing(&RUFF_CONFIGS) {
        report.should(
            "ruff-configured",
            "Ruff: configured",
            Outcome::issue(format!(
                "Ruff uses {name}, which takes the place of [tool.ruff] in pyproject.toml"
            )),
        );
        return;
    }

    let Some(tool) = &config.tool else {
        report.must(
            "ruff-configured",
            "Ruff: configured",
            Outcome::issue("No [tool] section in pyproject.toml"),
        );
        return;
    };

    let Some(ruff) = &tool.ruff else {
        report.must(
            "ruff-configured",
            "Ruff: configured",
            Outcome::issue("Missing [tool.ruff] in pyproject.toml"),
        );
        return;
    };

    report.must("ruff-configured", "Ruff: configured", Outcome::Pass);
    report.should(
        "ruff-line-length",
        format!("Ruff: line-length is {RUFF_LINE_LENGTH}"),
        check_line_length(ruff.line_length),
    );

    // Format settings
    if let Some(fmt) = &ruff.format {
        report.should(
            "ruff-format-configured",
            "Ruff format: configured",
            Outcome::Pass,
        );
        report.should(
            "ruff-format-lf",
            "Ruff format: LF line endings",
            fmt.line_ending.as_deref() == Some("lf"),
        );
    } else {
        report.should(
            "ruff-format-configured",
            "Ruff format: configured",
            Outcome::issue("Missing [tool.ruff.format]"),
        );
    }
}

pub(crate) fn check_ruff_lint(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    // check_ruff reports the file Ruff uses instead.
    if ctx.first_existing(&RUFF_CONFIGS).is_some() {
        return;
    }

    let lint = config
        .tool
        .as_ref()
        .and_then(|t| t.ruff.as_ref())
        .and_then(|r| r.lint.as_ref());

    let Some(lint) = lint else {
        report.must(
            "ruff-lint-configured",
            "Ruff lint: configured",
            Outcome::issue("Missing [tool.ruff.lint] in pyproject.toml"),
        );
        return;
    };

    report.must(
        "ruff-lint-configured",
        "Ruff lint: configured",
        Outcome::Pass,
    );
    report.should(
        "ruff-lint-preview",
        "Ruff lint: preview mode",
        lint.preview == Some(true),
    );
    report.should(
        "ruff-lint-rules",
        "Ruff lint: has rules selected",
        lint.has_rules(),
    );
    report.should(
        "ruff-lint-all",
        "Ruff lint: uses ALL rule",
        lint.selects_all(),
    );

    if lint.selects_all() {
        report.should(
            "ruff-lint-ignores",
            "Ruff lint: has ignore list",
            lint.has_ignores(),
        );
    }

    // Pydocstyle convention
    report.should(
        "ruff-lint-pydocstyle",
        "Ruff lint: pydocstyle convention",
        lint.pydocstyle
            .as_ref()
            .and_then(|p| p.convention.as_ref())
            .is_some(),
    );

    // Isort
    report.should(
        "ruff-lint-isort",
        "Ruff lint: isort configured",
        lint.isort.is_some(),
    );

    // Copyright notice
    report.should(
        "ruff-lint-copyright",
        "Ruff lint: copyright notice regex",
        lint.flake8_copyright
            .as_ref()
            .and_then(|c| c.notice_rgx.as_ref())
            .is_some(),
    );
}

//
// Type checking (Mypy)
//

pub(crate) fn check_mypy(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    if let Some(name) = ctx.first_existing(&MYPY_CONFIGS) {
        report.should(
            "mypy-configured",
            "mypy: configured",
            Outcome::issue(format!(
                "mypy uses {name}, which takes the place of [tool.mypy] in pyproject.toml"
            )),
        );
        return;
    }

    let mypy = config.tool.as_ref().and_then(|t| t.mypy.as_ref());

    let Some(mypy) = mypy else {
        report.must(
            "mypy-configured",
            "mypy: configured",
            Outcome::issue("Missing [tool.mypy] in pyproject.toml"),
        );
        return;
    };

    report.must("mypy-configured", "mypy: configured", Outcome::Pass);
    report.should(
        "mypy-strict",
        "mypy: strict mode",
        mypy.strict == Some(true),
    );
    report.should(
        "mypy-optional-checks",
        "mypy: the checks strict mode leaves out enabled",
        mypy.check_optional_checks(),
    );
}

//
// Testing (pytest)
//

/// The configuration files pytest prefers to `[tool.pytest]` in a `pyproject.toml` beside them, in its order.
const PYTEST_CONFIGS: [&str; 4] = ["pytest.toml", ".pytest.toml", "pytest.ini", ".pytest.ini"];

/// Check that pytest runs in strict mode, set in `[tool.pytest]`.
pub(crate) fn check_pytest(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    report.should(
        "pytest-strict",
        "pytest: strict mode ([tool.pytest] strict = true)",
        ctx.first_existing(&PYTEST_CONFIGS).map_or_else(
            || {
                config
                    .tool
                    .as_ref()
                    .and_then(|tool| tool.pytest.as_ref())
                    .map_or_else(
                        || Err("Missing [tool.pytest] in pyproject.toml".to_owned()),
                        PytestConfig::check_strict,
                    )
            },
            |name| {
                Err(format!(
                    "pytest uses {name}, which takes the place of [tool.pytest] in pyproject.toml"
                ))
            },
        ),
    );
}

//
// Pre-commit integration
//

/// The IDs of Python's language hooks, which the standard's hook table lists.
const LANGUAGE_HOOKS: [&str; 5] = ["uv-lock", "ruff-format", "ruff-check", "mypy", "pytest"];

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.check(
        "python-precommit-uv-lock",
        "Python pre-commit: uv-lock hook",
        config.check_commit_hook("uv-lock"),
    );

    report.check(
        "python-precommit-ruff-check",
        "Python pre-commit: ruff-check",
        config.check_commit_hook("ruff-check"),
    );
    report.check(
        "python-precommit-ruff-format",
        "Python pre-commit: ruff-format (--check)",
        config.check_formatter_hook("ruff-format", "ruff format", ruff_format_run),
    );

    // Mypy hook
    report.check(
        "python-precommit-mypy",
        "Python pre-commit: Mypy hook",
        config.check_commit_hook("mypy"),
    );

    report.check(
        "python-precommit-pytest",
        "Python pre-commit: pytest hook (with coverage)",
        config.commit_hooks("pytest").and_then(|hooks| {
            if hooks.into_iter().any(pytest_hook_covers) {
                Ok(())
            } else {
                Err(Issue::warn("The pytest hook doesn't pass --cov"))
            }
        }),
    );
    report.should(
        "python-precommit-local-hooks",
        "Python pre-commit: language hooks are local, unsupported and take no file names",
        config.check_language_hooks(&LANGUAGE_HOOKS),
    );
}

//
// CI integration
//

pub(crate) fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    report.must(
        "python-ci-uv-lock",
        "Python CI: uv.lock up to date",
        workflow.run_lines().any(|line| checks_uv_lock(&line)),
    );
    let unlocked = jobs_running_unlocked(workflow);
    report.should(
        "python-ci-uv-sync-locked",
        "Python CI: uv sync --locked before uv run",
        if unlocked.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Jobs that uv run without uv sync --locked first: {}",
                unlocked.join(", ")
            ))
        },
    );
    report.check(
        "python-ci-ruff-format",
        "Python CI: format check",
        workflow.check_formatter("ruff format", ruff_format_run),
    );
    report.must(
        "python-ci-ruff-check",
        "Python CI: lint check",
        workflow.runs_command("ruff check"),
    );
    report.must(
        "python-ci-mypy",
        "Python CI: type check",
        workflow.runs_command("mypy"),
    );
    report.check(
        "python-ci-pytest",
        "Python CI: tests (with coverage)",
        pytest_in_ci(workflow),
    );
}

/// Whether a command's words run `ruff format`, and if so how: with `--check`, which fails on unformatted files instead
/// of rewriting them, with `--diff`, which fails on them too, or neither.
fn ruff_format_run(words: &[&str]) -> Option<FormatterRun> {
    match program_args(words, "ruff")?.split_first() {
        Some((&"format", args)) => Some(FormatterRun::from_args(args, &["--check"], &["--diff"])),
        _ => None,
    }
}

/// uv's global options that take a value as the next word, such as the `--directory` that the hooks of a component in a
/// subdirectory pass before the subcommand.
const UV_GLOBAL_OPTIONS_WITH_VALUES: [&str; 8] = [
    "--allow-insecure-host",
    "--cache-dir",
    "--color",
    "--config-file",
    "--directory",
    "--project",
    "--python-fetch",
    "--python-preference",
];

/// The uv subcommand a command's words run, after any global options, and the arguments that follow it.
fn uv_subcommand<'a, 'b>(words: &'b [&'a str]) -> Option<(&'a str, &'b [&'a str])> {
    let mut rest = program_args(words, "uv")?;
    loop {
        let (&word, tail) = rest.split_first()?;
        if UV_GLOBAL_OPTIONS_WITH_VALUES.contains(&word) {
            rest = tail.get(1..)?;
        } else if word.starts_with('-') {
            rest = tail;
        } else {
            return Some((word, tail));
        }
    }
}

/// Whether a script line checks that `uv.lock` is up to date: `uv lock` with `--check`, or `--locked`, its older
/// spelling, which fail instead of changing the lock file.
fn checks_uv_lock(line: &str) -> bool {
    commands(line).any(|words| {
        matches!(uv_subcommand(&words), Some(("lock", args))
            if args.iter().any(|arg| matches!(*arg, "--check" | "--locked")))
    })
}

/// The jobs of `workflow` with a `uv run` that has no `--locked` of its own and comes before any `uv sync --locked`.
/// `uv run` locks the project first, so such a job would quietly update a stale `uv.lock` rather than fail. A `--locked`
/// anywhere after `run` counts, since telling uv's own options from those of the program it runs would need all of
/// their values.
fn jobs_running_unlocked(workflow: &Workflow) -> Vec<&str> {
    workflow
        .jobs
        .iter()
        .filter(|(_, job)| {
            let mut synced = false;
            for line in job.run_lines() {
                for words in commands(&line) {
                    match uv_subcommand(&words) {
                        Some(("sync", args)) if args.contains(&"--locked") => synced = true,
                        Some(("run", args)) if !synced && !args.contains(&"--locked") => {
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            false
        })
        .map(|(id, _)| id.as_str())
        .collect()
}

/// Whether a command's words run pytest, directly, through `uv run` or as `python -m pytest`, and if so, whether they
/// measure coverage with pytest-cov's `--cov`, with or without a source, and without the `--no-cov` that turns it off.
fn pytest_coverage(words: &[&str]) -> Option<bool> {
    let args = program_args(words, "pytest")?;
    Some(
        args.iter()
            .any(|arg| *arg == "--cov" || arg.starts_with("--cov="))
            && !args.contains(&"--no-cov"),
    )
}

/// Whether a script line runs pytest.
fn runs_pytest(line: &str) -> bool {
    commands(line).any(|words| pytest_coverage(&words).is_some())
}

/// Whether a pytest hook measures coverage, from its entry and the args pre-commit appends to it.
fn pytest_hook_covers(hook: &PrecommitHook) -> bool {
    hook.filter_map_commands("pytest", pytest_coverage)
        .contains(&true)
}

/// Check that some step runs pytest with coverage.
fn pytest_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let runs = workflow.filter_map_commands(pytest_coverage);
    if runs.contains(&true) {
        Ok(())
    } else if runs.is_empty() {
        Err(Issue::fail("No step runs pytest"))
    } else {
        Err(Issue::warn("No step runs pytest with --cov"))
    }
}

/// Whether the Python version setup-uv installs, `python_version`, is `min`: the same major and minor version, and the
/// same micro version if both give one.
fn is_min_python(python_version: &str, min: &Version) -> bool {
    let Ok(version) = python_version.trim().parse::<Version>() else {
        return false;
    };
    let ([major, minor, micro @ ..], [min_major, min_rest @ ..]) =
        (version.release(), min.release())
    else {
        return false;
    };
    major == min_major
        && *minor == min_rest.first().copied().unwrap_or_default()
        && match (micro.first(), min_rest.get(1)) {
            (Some(micro), Some(min_micro)) => micro == min_micro,
            _ => true,
        }
}

/// Check that a library's tests run in CI on the oldest Python its `requires-python` allows: some job sets that
/// version as setup-uv's `python-version` input and runs pytest.
fn min_python_tested(config: &PyprojectToml, workflow: &Workflow) -> Result<(), Issue> {
    let versions: Vec<_> = workflow
        .steps_using_in_jobs_running(SETUP_UV, runs_pytest)
        .filter_map(|step| step.input("python-version"))
        .collect();
    if versions.is_empty() {
        return Err(Issue::fail(
            "No job sets setup-uv's python-version input and runs pytest",
        ));
    }
    let min = config.min_python().map_err(Issue::warn)?;
    if versions.iter().any(|version| is_min_python(version, &min)) {
        Ok(())
    } else {
        Err(Issue::warn(format!(
            "No job tests with the oldest supported Python, {min} (the jobs set {})",
            versions.join(", ")
        )))
    }
}

/// Check that `audit.yaml` runs `uv audit`.
pub(crate) fn check_audit(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.audit_workflow() else {
        return;
    };

    report.must(
        "python-audit-uv",
        "Python audit: uv audit",
        workflow.runs_command("uv audit"),
    );
}

/// Check that a library's CI runs its tests on the oldest Python it supports.
pub(crate) fn check_min_python_job(
    project: &ProjectContext,
    ctx: &ComponentContext,
    report: &mut Report,
) {
    let (Some(config), ConfigStatus::Ok(workflow)) =
        (ctx.pyproject().as_ref_ok(), project.ci_workflow())
    else {
        return;
    };
    if config.is_application() {
        return;
    }

    report.check(
        "python-ci-min-python",
        "Python CI: tests on the oldest supported Python (library)",
        min_python_tested(config, workflow),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Component, Language};

    #[test]
    fn ruff_line_length_is_120() {
        assert_eq!(check_line_length(Some(120)), Ok(()));
        assert_eq!(
            check_line_length(Some(88)),
            Err("line-length is 88, not 120".to_owned())
        );
        assert_eq!(
            check_line_length(None),
            Err("Missing line-length".to_owned())
        );
    }

    #[test]
    fn git_archival_has_node_and_describe_name() {
        let standard = "node: $Format:%H$\nnode-date: $Format:%cI$\n\
                        describe-name: $Format:%(describe:tags=true,match=*[0-9]*)$\n";
        assert_eq!(check_git_archival_content(standard), Ok(()));
        // As git archive fills it in.
        assert_eq!(
            check_git_archival_content(
                "node: 4b772c14d2b5a3e8f0c1d6e7a8b9c0d1e2f3a4b5\nnode-date: 2026-11-06T12:00:00+00:00\n\
                 describe-name: v0.1.0-12-g4b772c1\n"
            ),
            Ok(())
        );
        assert_eq!(
            check_git_archival_content("node: $Format:%H$\nref-names: $Format:%D$\n"),
            Err(".git_archival.txt has no describe-name line".to_owned())
        );
        assert_eq!(
            check_git_archival_content("node-date: $Format:%cI$\n"),
            Err(".git_archival.txt has no node or describe-name line".to_owned())
        );
        assert_eq!(
            check_git_archival_content("node: $Format:%h$\ndescribe-name:$Format:%(describe)$\n"),
            Err(
                "node is $Format:%h$, not $Format:%H$; describe-name is $Format:%(describe)$, not \
                 $Format:%(describe:tags=true,match=*[0-9]*)$"
                    .to_owned()
            )
        );
    }

    #[test]
    fn uv_subcommands_follow_global_options() {
        fn subcommand(line: &str) -> Option<String> {
            let words: Vec<&str> = line.split_whitespace().collect();
            uv_subcommand(&words)
                .map(|(subcommand, args)| format!("{subcommand}: {}", args.join(" ")))
        }
        assert_eq!(
            subcommand("uv lock --check").as_deref(),
            Some("lock: --check")
        );
        assert_eq!(
            subcommand("uv --directory api --no-cache run --locked pytest").as_deref(),
            Some("run: --locked pytest")
        );
        assert_eq!(
            subcommand("~/.local/bin/uv --color=never sync").as_deref(),
            Some("sync: ")
        );
        assert_eq!(
            subcommand("uv --python-preference managed run pytest").as_deref(),
            Some("run: pytest")
        );
        assert_eq!(subcommand("uvx pre-commit run"), None);
        assert_eq!(subcommand("uv --directory"), None);

        for line in [
            "uv lock --check",
            "uv --directory api lock --locked",
            "cd api && uv lock --check",
        ] {
            assert!(checks_uv_lock(line), "{line}");
        }
        for line in [
            "uv lock",
            "uv sync --locked",
            "uv lock --upgrade && echo --check",
        ] {
            assert!(!checks_uv_lock(line), "{line}");
        }
    }

    #[test]
    fn uv_run_needs_a_locked_sync_first() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  synced:
    steps:
      - run: uv sync --locked
      - run: uv run ruff check
  locked-run:
    steps:
      - run: uv run --locked mypy .
  frozen:
    steps:
      - run: uv sync --frozen
      - run: uv run pytest
  late-sync:
    steps:
      - run: |
          uv run pytest
          uv sync --locked
  no-run:
    steps:
      - run: uv lock --check
",
        )
        .unwrap();
        assert_eq!(jobs_running_unlocked(&workflow), ["frozen", "late-sync"]);
    }

    #[test]
    fn min_python_jobs_set_the_lower_bound() {
        let min: Version = "3.12".parse().unwrap();
        for version in ["3.12", "3.12.0", "3.12.4", " 3.12 "] {
            assert!(is_min_python(version, &min), "{version}");
        }
        let micro: Version = "3.12.1".parse().unwrap();
        assert!(is_min_python("3.12", &micro));
        assert!(!is_min_python("3.12.0", &micro));
        for version in [
            "3.13",
            "3",
            "3.13t",
            "pypy3.12",
            ">=3.12",
            "${{ matrix.python }}",
        ] {
            assert!(!is_min_python(version, &min), "{version}");
        }

        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  pytest:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: 0.13.0
      - run: uv run pytest
  pytest-min-python:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          python-version: \"3.12\"
      - run: uv run pytest --cov=src/demo
",
        )
        .unwrap();
        let library = |requires: &str| -> PyprojectToml {
            toml::from_str(&format!("[project]\nrequires-python = \"{requires}\"\n")).unwrap()
        };
        assert_eq!(min_python_tested(&library(">=3.12"), &workflow), Ok(()));
        assert_eq!(
            min_python_tested(&library(">=3.11"), &workflow),
            Err(Issue::warn(
                "No job tests with the oldest supported Python, 3.11 (the jobs set 3.12)"
            ))
        );
        assert_eq!(
            min_python_tested(&library(">=3.11"), &Workflow::default()),
            Err(Issue::fail(
                "No job sets setup-uv's python-version input and runs pytest"
            ))
        );
        for line in ["uv run pytest", "python -m pytest -q", ".venv/bin/pytest"] {
            assert!(runs_pytest(line), "{line}");
        }
        for line in ["uv run pytest-benchmark", "uv run mypy ."] {
            assert!(!runs_pytest(line), "{line}");
        }
    }

    #[test]
    fn tool_config_files_are_found_as_the_tools_find_them() {
        // The ruff-configured, mypy-configured and uv-config messages for a component, given its files.
        let messages = |files: &[&str]| {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("pyproject.toml"),
                "[project]\nname = \"x\"\n",
            )
            .unwrap();
            for file in files {
                std::fs::write(dir.path().join(file), "").unwrap();
            }
            let component = Component {
                language: Language::Python,
                path: std::path::PathBuf::new(),
            };
            let project = ProjectContext::new(dir.path().to_path_buf());
            let ctx = ComponentContext::new(&project, &component);
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_ruff(&ctx, &mut report);
            check_mypy(&ctx, &mut report);
            check_uv_config(&ctx, &mut report);
            report
                .results
                .iter()
                .map(|result| result.message.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            messages(&[
                "ruff.toml",
                ".ruff.toml",
                "mypy.ini",
                ".mypy.ini",
                "uv.toml"
            ]),
            [
                "Ruff uses .ruff.toml, which takes the place of [tool.ruff] in pyproject.toml",
                "mypy uses mypy.ini, which takes the place of [tool.mypy] in pyproject.toml",
                "uv uses uv.toml, which takes the place of the settings in [tool.uv] in pyproject.toml",
            ]
        );
        assert_eq!(
            messages(&[".mypy.ini"]),
            [
                "No [tool] section in pyproject.toml".to_owned(),
                "mypy uses .mypy.ini, which takes the place of [tool.mypy] in pyproject.toml"
                    .to_owned(),
                String::new(),
            ]
        );
    }

    #[test]
    fn pytest_config_files_take_the_place_of_the_table() {
        let message = |files: &[&str]| {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("pyproject.toml"),
                "[project]\nname = \"x\"\n\n[tool.pytest]\nstrict = true\n",
            )
            .unwrap();
            for file in files {
                std::fs::write(dir.path().join(file), "").unwrap();
            }
            let component = Component {
                language: Language::Python,
                path: std::path::PathBuf::new(),
            };
            let project = ProjectContext::new(dir.path().to_path_buf());
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_pytest(&ComponentContext::new(&project, &component), &mut report);
            report.results.first().map(|result| result.message.clone())
        };
        assert_eq!(message(&[]), Some(String::new()));
        assert_eq!(
            message(&[".pytest.ini", "pytest.ini", ".pytest.toml"]),
            Some("pytest uses .pytest.toml, which takes the place of [tool.pytest] in pyproject.toml".to_owned())
        );
    }

    #[test]
    fn pytest_measures_coverage_with_cov() {
        let coverage = |line: &str| {
            let words: Vec<&str> = line.split_whitespace().collect();
            pytest_coverage(&words)
        };
        for line in [
            "uv run pytest --cov=src/demo --cov-report=term-missing",
            "python -m pytest -q --cov",
            ".venv/bin/pytest --cov src/demo",
        ] {
            assert_eq!(coverage(line), Some(true), "{line}");
        }
        for line in [
            "uv run pytest",
            "uv run pytest --cov-report=term-missing",
            "uv run pytest --cov=src/demo --no-cov",
        ] {
            assert_eq!(coverage(line), Some(false), "{line}");
        }
        assert_eq!(coverage("uv run --cov pytest-benchmark"), None);

        let hook = |entry: &str, args: &[&str]| PrecommitHook {
            id: "pytest".to_owned(),
            entry: Some(entry.to_owned()),
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            ..PrecommitHook::default()
        };
        assert!(pytest_hook_covers(&hook(
            "uv run pytest --cov=src/demo",
            &[]
        )));
        assert!(pytest_hook_covers(&hook(
            "uv run pytest",
            &["--cov=src/demo"]
        )));
        assert!(!pytest_hook_covers(&hook("uv run pytest", &[])));
        let remote = PrecommitHook {
            args: vec!["--cov".to_owned()],
            ..PrecommitHook::default()
        };
        assert!(pytest_hook_covers(&remote));

        let workflow = |run: &str| -> Workflow {
            serde_saphyr::from_str(&format!("jobs:\n  test:\n    steps:\n      - run: {run}\n"))
                .unwrap()
        };
        assert_eq!(
            pytest_in_ci(&workflow("uv run pytest && uv run pytest --cov")),
            Ok(())
        );
        assert_eq!(
            pytest_in_ci(&workflow("uv run pytest")),
            Err(Issue::warn("No step runs pytest with --cov"))
        );
        assert_eq!(
            pytest_in_ci(&workflow("uv run mypy .")),
            Err(Issue::fail("No step runs pytest"))
        );
    }

    #[test]
    fn ruff_format_in_ci_checks_rather_than_writes() {
        let checks = |line: &str| ruff_format_run(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "uv run ruff format --check",
            "uvx ruff format --check src",
            "ruff format . --check",
        ] {
            assert_eq!(checks(line), Some(FormatterRun::Checks), "{line}");
        }
        assert_eq!(
            checks("ruff format --diff ."),
            Some(FormatterRun::ChecksWith("--diff"))
        );
        assert_eq!(checks("uv run ruff format"), Some(FormatterRun::Rewrites));
        for line in ["uv run ruff check --check", "uv run mypy ."] {
            assert_eq!(checks(line), None, "{line}");
        }
    }
}

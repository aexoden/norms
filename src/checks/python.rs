// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Python-specific checks.

use pep508_rs::pep440_rs::Version;

use crate::config::{
    PYTHON_VERSION, PrecommitHook, PyprojectToml, Workflow, check_python_version_exact,
    check_release_age_value, commands, program_args,
};
use crate::context::{ComponentContext, ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

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
        report.should(
            "pyproject-license-texts",
            "pyproject.toml: license has its texts in LICENSES/",
            license.as_str().map_or_else(
                || Err("Not an SPDX expression string".to_owned()),
                |expression| ctx.license_files().check_expression(expression),
            ),
        );
    }
}

/// Check that the component pins its development Python with an exact version in its own `.python-version`.
pub(crate) fn check_python_version(ctx: &ComponentContext, report: &mut Report) {
    report.should(
        "python-version-exists",
        "Python: .python-version",
        file_exists(&ctx.path().join(PYTHON_VERSION)),
    );
    // A pin that isn't a plain version, such as `cpython@3.15`, is reported here rather than as invalid: uv accepts
    // it, but it isn't the exact version the standards ask for.
    let exact = match ctx.python_version() {
        ConfigStatus::NotFound => return,
        ConfigStatus::ParseError { error, .. } => Err(error.clone()),
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

//
// Project layout
//

pub(crate) fn check_src_layout(ctx: &ComponentContext, report: &mut Report) {
    let src_path = ctx.path().join("src");

    if !src_path.is_dir() {
        report.should(
            "python-src-layout",
            "Python: src/ directory",
            Outcome::fail("Consider using a src/ layout"),
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
        has_packages,
    );
}

//
// Dependency management
//

pub(crate) fn check_uv_lock(ctx: &ComponentContext, report: &mut Report) {
    report.should(
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
            Outcome::fail("Consider using dependency-groups for dev dependencies"),
        );
    }
}

//
// Ruff
//

pub(crate) fn check_ruff(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    let Some(tool) = &config.tool else {
        report.must(
            "ruff-configured",
            "Ruff: configured",
            Outcome::fail("No [tool] section in pyproject.toml"),
        );
        return;
    };

    let Some(ruff) = &tool.ruff else {
        report.must(
            "ruff-configured",
            "Ruff: configured",
            Outcome::fail("Missing [tool.ruff] in pyrpoject.toml"),
        );
        return;
    };

    report.must("ruff-configured", "Ruff: configured", Outcome::Pass);
    report.should(
        "ruff-line-length",
        "Ruff: line-length set",
        ruff.line_length.is_some(),
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
            Outcome::fail("Consider configuring [tool.ruff.format]"),
        );
    }
}

pub(crate) fn check_ruff_lint(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    let lint = config
        .tool
        .as_ref()
        .and_then(|t| t.ruff.as_ref())
        .and_then(|r| r.lint.as_ref());

    let Some(lint) = lint else {
        report.should(
            "ruff-lint-configured",
            "Ruff lint: configured",
            Outcome::fail("Consider configuring [tool.ruff.lint]"),
        );
        return;
    };

    report.should(
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

    let mypy = config.tool.as_ref().and_then(|t| t.mypy.as_ref());

    let Some(mypy) = mypy else {
        report.must(
            "mypy-configured",
            "mypy: configured",
            Outcome::fail("Missing [tool.mypy] in pyproject.toml"),
        );
        return;
    };

    report.must("mypy-configured", "mypy: configured", Outcome::Pass);
    report.should(
        "mypy-strict",
        "mypy: strict mode",
        mypy.strict == Some(true),
    );
}

//
// Pre-commit integration
//

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.should(
        "python-precommit-uv-lock",
        "Python pre-commit: uv-lock hook",
        config.has_hook("uv-lock"),
    );

    report.should(
        "python-precommit-ruff-check",
        "Python pre-commit: ruff-check",
        config.has_hook("ruff-check"),
    );
    report.should(
        "python-precommit-ruff-format",
        "Python pre-commit: ruff-format",
        config.has_hook("ruff-format"),
    );

    // Mypy hook
    report.should(
        "python-precommit-mypy",
        "Python pre-commit: Mypy hook",
        config.has_hook("mypy"),
    );

    report.should(
        "python-precommit-pytest",
        "Python pre-commit: pytest hook (with coverage)",
        if !config.has_hook("pytest") {
            Err("No pytest hook".to_owned())
        } else if config.hooks("pytest").any(pytest_hook_covers) {
            Ok(())
        } else {
            Err("The pytest hook doesn't pass --cov".to_owned())
        },
    );
}

//
// CI integration
//

pub(crate) fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    report.should(
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
    report.should(
        "python-ci-ruff-format",
        "Python CI: format check",
        workflow.runs_command("ruff format"),
    );
    report.should(
        "python-ci-ruff-check",
        "Python CI: lint check",
        workflow.runs_command("ruff check"),
    );
    report.should(
        "python-ci-mypy",
        "Python CI: type check",
        workflow.runs_command("mypy"),
    );
    report.should(
        "python-ci-pytest",
        "Python CI: tests (with coverage)",
        pytest_in_ci(workflow),
    );
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
fn pytest_in_ci(workflow: &Workflow) -> Result<(), String> {
    let runs = workflow.filter_map_commands(pytest_coverage);
    if runs.contains(&true) {
        Ok(())
    } else if runs.is_empty() {
        Err("No step runs pytest".to_owned())
    } else {
        Err("No step runs pytest with --cov".to_owned())
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
fn min_python_tested(config: &PyprojectToml, workflow: &Workflow) -> Result<(), String> {
    let versions: Vec<_> = workflow
        .steps_using_in_jobs_running("astral-sh/setup-uv", runs_pytest)
        .filter_map(|step| step.input("python-version"))
        .collect();
    if versions.is_empty() {
        return Err("No job sets setup-uv's python-version input and runs pytest".to_owned());
    }
    let min = config.min_python()?;
    if versions.iter().any(|version| is_min_python(version, &min)) {
        Ok(())
    } else {
        Err(format!(
            "No job tests with the oldest supported Python, {min} (the jobs set {})",
            versions.join(", ")
        ))
    }
}

/// Check that `audit.yaml` runs `uv audit`.
pub(crate) fn check_audit(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.audit_workflow() else {
        return;
    };

    report.should(
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

    report.should(
        "python-ci-min-python",
        "Python CI: tests on the oldest supported Python (library)",
        min_python_tested(config, workflow),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Err(
                "No job tests with the oldest supported Python, 3.11 (the jobs set 3.12)"
                    .to_owned()
            )
        );
        assert_eq!(
            min_python_tested(&library(">=3.11"), &Workflow::default()),
            Err("No job sets setup-uv's python-version input and runs pytest".to_owned())
        );
        for line in ["uv run pytest", "python -m pytest -q", ".venv/bin/pytest"] {
            assert!(runs_pytest(line), "{line}");
        }
        for line in ["uv run pytest-benchmark", "uv run mypy ."] {
            assert!(!runs_pytest(line), "{line}");
        }
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
            Err("No step runs pytest with --cov".to_owned())
        );
        assert_eq!(
            pytest_in_ci(&workflow("uv run mypy .")),
            Err("No step runs pytest".to_owned())
        );
    }
}

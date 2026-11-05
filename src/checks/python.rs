// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Python-specific checks.

use pep508_rs::pep440_rs::Version;

use crate::config::{PYTHON_VERSION, PyprojectToml, Workflow, check_release_age_value};
use crate::context::{ComponentContext, ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

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

/// Check that the Python classifiers match `requires-python`, up to the version in `.python-version`.
pub(crate) fn check_python_classifiers(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };
    let pinned = match ctx.python_version() {
        ConfigStatus::Ok(version) => Some(version),
        ConfigStatus::NotFound => None,
        ConfigStatus::ParseError { error, .. } => {
            report.should(
                "pyproject-python-classifiers",
                "pyproject.toml: Python classifiers match requires-python",
                Outcome::fail(format!("Invalid {PYTHON_VERSION}: {error}")),
            );
            return;
        }
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

    // Ruff hooks
    let has_ruff = config.has_repo_containing("ruff") || config.has_repo_containing("local");
    report.should(
        "python-precommit-ruff",
        "Python pre-commit: Ruff hooks",
        has_ruff,
    );

    if has_ruff {
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
    }

    // Mypy hook
    report.should(
        "python-precommit-mypy",
        "Python pre-commit: Mypy hook",
        config.has_hook("mypy"),
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
}

/// Whether a script line runs pytest, directly, through `uv run` or as `python -m pytest`.
fn runs_pytest(line: &str) -> bool {
    line.split_whitespace()
        .any(|word| word == "pytest" || word.ends_with("/pytest"))
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
}

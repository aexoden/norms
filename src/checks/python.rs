// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Python-specific checks.

use crate::context::{ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

//
// Project configuration
//

pub fn check_pyproject(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("pyproject.toml", ctx.pyproject()) else {
        return;
    };

    report.must(
        "pyproject.toml: has [project] section",
        config.project.is_some(),
    );
}

//
// Project layout
//

pub fn check_src_layout(ctx: &ProjectContext, report: &mut Report) {
    let src_path = ctx.path().join("src");

    if !src_path.is_dir() {
        report.should(
            "Python: src/ directory",
            Outcome::fail("Consider using a src/ layout"),
        );

        return;
    }
    report.should("Python: src/ directory", Outcome::Pass);

    let has_packages = src_path
        .read_dir()
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .any(|entry| entry.path().is_dir() && entry.path().join("__init__.py").exists());

    report.should("Python: src/ contains packages", has_packages);
}

//
// Dependency management
//

pub fn check_uv_lock(ctx: &ProjectContext, report: &mut Report) {
    report.should("Python: uv.lock", file_exists(&ctx.path().join("uv.lock")));
}

pub fn check_dependency_groups(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    if let Some(groups) = &config.dependency_groups {
        report.should("Python: has dependency-groups", Outcome::Pass);
        report.should(
            "Python: has dev dependency group",
            groups.contains_key("dev"),
        );
    } else {
        report.should(
            "Python: has dependency-groups",
            Outcome::fail("Consider using dependency-groups for dev dependencies"),
        );
    }
}

//
// Ruff
//

pub fn check_ruff(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    let Some(tool) = &config.tool else {
        report.must(
            "Ruff: configured",
            Outcome::fail("No [tool] section in pyproject.toml"),
        );
        return;
    };

    let Some(ruff) = &tool.ruff else {
        report.must(
            "Ruff: configured",
            Outcome::fail("Missing [tool.ruff] in pyrpoject.toml"),
        );
        return;
    };

    report.must("Ruff: configured", Outcome::Pass);
    report.should("Ruff: line-length set", ruff.line_length.is_some());

    // Format settings
    if let Some(fmt) = &ruff.format {
        report.should("Ruff format: configured", Outcome::Pass);
        report.should(
            "Ruff format: LF line endings",
            fmt.line_ending.as_deref() == Some("lf"),
        );
    } else {
        report.should(
            "Ruff format: configured",
            Outcome::fail("Consider configuring [tool.ruff.format]"),
        );
    }
}

pub fn check_ruff_lint(ctx: &ProjectContext, report: &mut Report) {
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
            "Ruff lint: configured",
            Outcome::fail("Consider configuring [tool.ruff.lint]"),
        );
        return;
    };

    report.should("Ruff lint: configured", Outcome::Pass);
    report.should("Ruff lint: preview mode", lint.preview == Some(true));
    report.should("Ruff lint: has rules selected", lint.has_rules());
    report.should("Ruff lint: uses ALL rule", lint.selects_all());

    if lint.selects_all() {
        report.should("Ruff lint: has ignore list", lint.has_ignores());
    }

    // Pydocstyle convention
    report.should(
        "Ruff lint: pydocstyle convention",
        lint.pydocstyle
            .as_ref()
            .and_then(|p| p.convention.as_ref())
            .is_some(),
    );

    // Isort
    report.should("Ruff lint: isort configured", lint.isort.is_some());

    // Copyright notice
    report.should(
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

pub fn check_mypy(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.pyproject().as_ref_ok() else {
        return;
    };

    let mypy = config.tool.as_ref().and_then(|t| t.mypy.as_ref());

    let Some(mypy) = mypy else {
        report.must(
            "mypy: configured",
            Outcome::fail("Missing [tool.mypy] in pyproject.toml"),
        );
        return;
    };

    report.must("mypy: configured", Outcome::Pass);
    report.should("mypy: strict mode", mypy.strict == Some(true));
}

//
// Pre-commit integration
//

pub fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    // Ruff hooks
    let has_ruff = config.has_repo_containing("ruff");
    report.should("Python pre-commit: Ruff hooks", has_ruff);

    if has_ruff {
        report.should(
            "Python pre-commit: ruff-check",
            config.has_hook("ruff-check"),
        );
        report.should(
            "Python pre-commit: ruff-format",
            config.has_hook("ruff-format"),
        );
    }

    // Mypy hook
    report.should("Python pre-commit: Mypy hook", config.has_hook("mypy"));
}

//
// Devbox integration
//

pub fn check_devbox_uv(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.devbox().as_ref_ok() else {
        return;
    };

    report.should("Python devbox: includes uv", config.has_package("uv"));
}

//
// CI integration
//

pub fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(content) = ctx.ci_workflow() else {
        return;
    };

    let lower = content.to_lowercase();
    report.should("Python CI: format check", lower.contains("ruff format"));
    report.should("Python CI: lint check", lower.contains("ruff check"));
    report.should("Python CI: type check", lower.contains("mypy"));
}

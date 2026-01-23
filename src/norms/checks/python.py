# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Python-specific checks."""

from __future__ import annotations

import tomllib

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

from norms.checks import register_language
from norms.checks.base import check_file_exists
from norms.models import Language, Status, VerificationReport

# -----------------------------------------------------------------------------
# Project Configuration
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_pyproject_toml(path: Path, report: VerificationReport) -> None:
    """Check for the presence and validity of pyproject.toml."""
    pyproject_path = path / "pyproject.toml"
    status, message = check_file_exists(pyproject_path)

    if status != Status.PASS:
        report.add("pyproject.toml exists", Status.FAIL, message)
        return

    report.add("pyproject.toml exists", Status.PASS)

    try:
        with pyproject_path.open("rb") as f:
            config = tomllib.load(f)
        report.add("pyproject.toml valid TOML", Status.PASS)
    except tomllib.TOMLDecodeError as e:
        report.add("pyproject.toml valid TOML", Status.FAIL, f"Invalid TOML: {e}")
        return

    # Check for [project] section
    project = config.get("project", {})

    if project:
        report.add("pyproject.toml has [project] section", Status.PASS)
    else:
        report.add("pyproject.toml has [project] section", Status.FAIL, "Missing [project] section")


# -----------------------------------------------------------------------------
# Project Layout
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_src_layout(path: Path, report: VerificationReport) -> None:
    """Check for the presence of a src/ directory for source code."""
    src_path = path / "src"
    status, _ = check_file_exists(src_path)

    if status == Status.PASS and src_path.is_dir():
        report.add("src/ directory exists", Status.PASS)

        packages = [p for p in src_path.iterdir() if p.is_dir() and (p / "__init__.py").exists()]

        if packages:
            report.add("src/ contains packages", Status.PASS)
        else:
            report.add("src/ contains packages", Status.WARN, "No packages found in src/ directory")
    else:
        report.add("src/ directory exists", Status.WARN, "Consider using a src/ layout for source code")


# -----------------------------------------------------------------------------
# Dependency Management
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_uv_lock(path: Path, report: VerificationReport) -> None:
    """Check for the presence of uv.lock file for projects using uv."""
    uv_lock_path = path / "uv.lock"
    status, _ = check_file_exists(uv_lock_path)

    if status == Status.PASS:
        report.add("uv.lock file exists", Status.PASS)
    else:
        report.add("uv.lock file exists", Status.WARN, "Consider adding uv.lock for dependency locking")


@register_language(Language.PYTHON)
def check_dependency_groups(path: Path, report: VerificationReport) -> None:
    """Check for development dependency groups in pyproject.toml."""
    pyproject_path = path / "pyproject.toml"
    status, _ = check_file_exists(pyproject_path)

    if status != Status.PASS:
        return  # pyproject.toml existence already reported

    try:
        with pyproject_path.open("rb") as f:
            config = tomllib.load(f)
    except tomllib.TOMLDecodeError:
        return  # Already reported in another check

    dep_groups = config.get("dependency-groups", {})
    if dep_groups:
        report.add("Has dependency-groups", Status.PASS)

        if "dev" in dep_groups:
            report.add("Has dev dependency group", Status.PASS)
        else:
            report.add("Has dev dependency group", Status.WARN, "Consider adding a 'dev' dependency group")
    else:
        report.add("Has dependency-groups", Status.WARN, "Consider using dependency-groups for dev dependencies")


# -----------------------------------------------------------------------------
# Linting and Formatting
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_ruff(path: Path, report: VerificationReport) -> None:
    """Check for Ruff configuration with proper settings."""
    pyproject_path = path / "pyproject.toml"
    status, _ = check_file_exists(pyproject_path)

    if status != Status.PASS:
        return  # pyproject.toml existence already reported

    try:
        with (pyproject_path).open("rb") as f:
            config = tomllib.load(f)
    except tomllib.TOMLDecodeError:
        return  # Already reported in another check

    ruff_config = config.get("tool", {}).get("ruff", {})

    if not ruff_config:
        report.add("Ruff is configured", Status.FAIL, "Ruff is not configured in pyproject.toml")
        return

    report.add("Ruff is configured", Status.PASS)

    # Check for line-length
    if "line-length" in ruff_config:
        report.add("Ruff has line-length configured", Status.PASS)
    else:
        report.add("Ruff has line-length configured", Status.WARN, "Consider setting line-length")

    # Check format settings
    format_config = ruff_config.get("format", {})
    if format_config:
        report.add("Ruff format is configured", Status.PASS)

        # Check for LF line endings
        if format_config.get("line-ending") == "lf":
            report.add("Ruff format uses LF line endings", Status.PASS)
        else:
            report.add("Ruff format uses LF line endings", Status.WARN, "Consider setting line-ending = 'lf'")
    else:
        report.add("Ruff format is configured", Status.WARN, "Consider configuring [tool.ruff.format]")


@register_language(Language.PYTHON)
def check_ruff_lint(path: Path, report: VerificationReport) -> None:  # noqa: C901, PLR0912 (series of independent checks)
    """Check for Ruff linting rules configuration."""
    pyproject_path = path / "pyproject.toml"
    status, _ = check_file_exists(pyproject_path)

    if status != Status.PASS:
        return  # pyproject.toml existence already reported

    try:
        with (pyproject_path).open("rb") as f:
            config = tomllib.load(f)
    except tomllib.TOMLDecodeError:
        return  # Already reported in another check

    ruff_config = config.get("tool", {}).get("ruff", {})
    lint_config = ruff_config.get("lint", {})

    if not lint_config:
        report.add("Ruff lint is configured", Status.WARN, "Consider configuring [tool.ruff.lint]")
        return

    report.add("Ruff lint is configured", Status.PASS)

    # Check for preview mode
    if lint_config.get("preview"):
        report.add("Ruff lint preview mode enabled", Status.PASS)
    else:
        report.add("Ruff lint preview mode enabled", Status.WARN, "Consider enabling preview mode for latest rules")

    # Check for select rules
    select = lint_config.get("select", [])
    if not select:
        report.add("Ruff lint has rules selected", Status.WARN, "No lint rules selected")
        return

    report.add("Ruff lint has rules selected", Status.PASS)

    # Check for ALL rule
    if "ALL" in select:
        report.add("Ruff lint uses ALL rule", Status.PASS)
    else:
        report.add("Ruff lint uses ALL rule", Status.WARN, "Consider enabling the ALL rule")

    # Check for ignore list (expected when using ALL)
    ignore = lint_config.get("ignore", [])
    if "ALL" in select and ignore:
        report.add("Ruff lint has ignore list", Status.PASS)
    elif "ALL" in select:
        report.add("Ruff lint has ignore list", Status.WARN, "Consider adding ignore list for rules that conflict")

    # Check for pydocstyle convention
    pydocstyle_config = lint_config.get("pydocstyle", {})
    if pydocstyle_config.get("convention"):
        report.add("Ruff pydocstyle convention set", Status.PASS)
    else:
        report.add(
            "Ruff pydocstyle convention set", Status.WARN, "Consider setting pydocstyle convention (e.g., 'google')"
        )

    # Check for isort configuration
    isort_config = lint_config.get("isort", {})
    if isort_config:
        report.add("Ruff isort is configured", Status.PASS)
    else:
        report.add("Ruff isort is configured", Status.WARN, "Consider configuring isort settings")

    # Check for flake8-copyright
    copyright_config = lint_config.get("flake8-copyright", {})
    if copyright_config.get("notice-rgx"):
        report.add("Ruff copyright notice configured", Status.PASS)
    else:
        report.add("Ruff copyright notice configured", Status.WARN, "Consider configuring copyright notice regex")


# -----------------------------------------------------------------------------
# Type Checking (mypy)
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_mypy(path: Path, report: VerificationReport) -> None:
    """Check for the presence of mypy configuration."""
    pyproject_path = path / "pyproject.toml"
    status, _ = check_file_exists(pyproject_path)

    if status != Status.PASS:
        return  # pyproject.toml existence already reported

    try:
        with (pyproject_path).open("rb") as f:
            config = tomllib.load(f)
    except tomllib.TOMLDecodeError:
        return  # Already reported in another check

    mypy_config = config.get("tool", {}).get("mypy", {})

    if not mypy_config:
        report.add("mypy is configured", Status.FAIL, "mypy is not configured in pyproject.toml")
        return

    report.add("mypy is configured", Status.PASS)

    # Check for strict mode
    if mypy_config.get("strict"):
        report.add("mypy strict mode enabled", Status.PASS)
    else:
        report.add("mypy strict mode enabled", Status.WARN, "Consider enabling strict mode")


# -----------------------------------------------------------------------------
# Pre-commit Integration
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_python_precommit_hooks(path: Path, report: VerificationReport) -> None:
    """Check for Python-specific pre-commit hooks."""
    precommit_path = path / ".pre-commit-config.yaml"
    status, _ = check_file_exists(precommit_path)

    if status != Status.PASS:
        return

    content = precommit_path.read_text()

    # Check for ruff hooks
    if "ruff" in content:
        report.add("Pre-commit has Ruff hooks", Status.PASS)

        # Check for both ruff-check and ruff-format
        if "ruff-check" in content:
            report.add("Pre-commit has ruff-check", Status.PASS)
        else:
            report.add("Pre-commit has ruff-check", Status.WARN, "Consider adding ruff-check hook")

        if "ruff-format" in content:
            report.add("Pre-commit has ruff-format", Status.PASS)
        else:
            report.add("Pre-commit has ruff-format", Status.WARN, "Consider adding ruff-format hook")
    else:
        report.add("Pre-commit has Ruff hooks", Status.WARN, "Consider adding Ruff pre-commit hooks")

    # Check for mypy hook
    if "mypy" in content:
        report.add("Pre-commit has mypy hook", Status.PASS)
    else:
        report.add("Pre-commit has mypy hook", Status.WARN, "Consider adding mypy pre-commit hook")


# -----------------------------------------------------------------------------
# Devbox Integration
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_python_devbox(path: Path, report: VerificationReport) -> None:
    """Check for Python-related packages in Devbox configuration."""
    devbox_path = path / "devbox.json"
    status, _ = check_file_exists(devbox_path)

    if status != Status.PASS:
        return

    content = devbox_path.read_text()

    # Check for uv in devbox packages
    if "uv@" in content or '"uv"' in content:
        report.add("Devbox includes uv", Status.PASS)
    else:
        report.add("Devbox includes uv", Status.WARN, "Consider adding uv to Devbox packages")


# -----------------------------------------------------------------------------
# CI Integration
# -----------------------------------------------------------------------------


@register_language(Language.PYTHON)
def check_python_ci(path: Path, report: VerificationReport) -> None:
    """Check for Python-related CI steps."""
    ci_path = path / ".github" / "workflows" / "ci.yaml"
    status, _ = check_file_exists(ci_path)

    if status != Status.PASS:
        return

    content = ci_path.read_text()

    # Check for format check
    if "ruff format" in content.lower():
        report.add("CI checks formatting", Status.PASS)
    else:
        report.add("CI checks formatting", Status.WARN, "Consider adding format check to CI")

    # Check for lint check
    if "ruff check" in content.lower():
        report.add("CI runs linting", Status.PASS)
    else:
        report.add("CI runs linting", Status.WARN, "Consider adding lint check to CI")

    # Check for type check
    if "mypy" in content.lower():
        report.add("CI runs type checking", Status.PASS)
    else:
        report.add("CI runs type checking", Status.WARN, "Consider adding type check to CI")

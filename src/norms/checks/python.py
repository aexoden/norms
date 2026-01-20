# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Python-specific checks."""

from __future__ import annotations

import tomllib

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

from norms.checks import register_language
from norms.models import Language, Status, VerificationReport


@register_language(Language.PYTHON)
def check_pyproject_toml(path: Path, report: VerificationReport) -> None:
    """Check for the presence and validity of pyproject.toml."""
    pyproject_path = path / "pyproject.toml"

    if not pyproject_path.exists():
        report.add("pyproject.toml exists", Status.FAIL, "Missing pyproject.toml")
        return

    report.add("pyproject.toml exists", Status.PASS)

    try:
        with pyproject_path.open("rb") as f:
            tomllib.load(f)
        report.add("pyproject.toml valid TOML", Status.PASS)
    except tomllib.TOMLDecodeError as e:
        report.add("pyproject.toml valid TOML", Status.FAIL, f"Invalid TOML: {e}")
        return


@register_language(Language.PYTHON)
def check_src_layout(path: Path, report: VerificationReport) -> None:
    """Check for the presence of a src/ directory for source code."""
    src_path = path / "src"

    if src_path.exists() and src_path.is_dir():
        report.add("src/ directory exists", Status.PASS)
    else:
        report.add("src/ directory exists", Status.WARN, "Consider using a src/ layout for source code")


@register_language(Language.PYTHON)
def check_uv_lock(path: Path, report: VerificationReport) -> None:
    """Check for the presence of uv.lock file for projects using uv."""
    uv_lock_path = path / "uv.lock"

    if uv_lock_path.exists():
        report.add("uv.lock file exists", Status.PASS)
    else:
        report.add("uv.lock file exists", Status.WARN, "Consider adding uv.lock for dependency locking")


@register_language(Language.PYTHON)
def check_ruff(path: Path, report: VerificationReport) -> None:
    """Check for the presence of ruff configuration."""
    with (path / "pyproject.toml").open("rb") as f:
        config = tomllib.load(f)

    ruff_config = config.get("tool", {}).get("ruff", {})

    if not ruff_config:
        report.add("Ruff configured", Status.FAIL, "Ruff is not configured")
        return

    report.add("Ruff configured", Status.PASS)

    lint_config = ruff_config.get("lint", {})
    select = lint_config.get("select", [])

    if not select:
        report.add("Ruff lint rules selected", Status.WARN, "No lint rules selected in Ruff configuration")
        return

    report.add("Ruff lint rules selected", Status.PASS)

    if "ALL" not in select:
        report.add("Ruff ALL rule selected", Status.WARN, "Consider enabling the ALL rule in Ruff configuration")
        return

    report.add("Ruff ALL rule selected", Status.PASS)


@register_language(Language.PYTHON)
def check_mypy(path: Path, report: VerificationReport) -> None:
    """Check for the presence of mypy configuration."""
    with (path / "pyproject.toml").open("rb") as f:
        config = tomllib.load(f)

    mypy_config = config.get("tool", {}).get("mypy", {})

    if not mypy_config:
        report.add("mypy configured", Status.FAIL, "mypy is not configured")
        return

    report.add("mypy configured", Status.PASS)

    strict = mypy_config.get("strict", False)

    if not strict:
        report.add("mypy strict mode", Status.WARN, "Consider enabling strict mode in mypy configuration")
        return

    report.add("mypy strict mode", Status.PASS)

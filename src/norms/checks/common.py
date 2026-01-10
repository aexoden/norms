# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Common checks that apply to all projects."""

from __future__ import annotations

import json

from typing import TYPE_CHECKING

from norms.checks import register_common
from norms.checks.base import check_file_exists
from norms.models import Status, VerificationReport

if TYPE_CHECKING:
    from pathlib import Path


@register_common
def check_required_files(path: Path, report: VerificationReport) -> None:
    """Check for required configuration files that don't have individual checks."""
    required_files = [
        (".gitattributes", "Git attributes"),
        (".gitignore", "Git ignore"),
        ("LICENSE", "License file"),
        ("README.md", "README"),
    ]

    for filename, description in required_files:
        status, message = check_file_exists(path, filename)
        report.add(description, status, message)


@register_common
def check_devbox(path: Path, report: VerificationReport) -> None:
    """Check for Devbox configuration."""
    devbox_path = path / "devbox.json"

    if devbox_path.exists():
        report.add("Devbox configuration", Status.PASS)

        try:
            devbox_config = json.loads(devbox_path.read_text())

            if devbox_config.get("packages"):
                report.add("Devbox has packages", Status.PASS)
            else:
                report.add("Devbox has packages", Status.WARN, "No packages defined")
        except json.JSONDecodeError:
            report.add("Devbox valid JSON", Status.FAIL, "Invalid JSON")
    else:
        report.add("Devbox configuration", Status.FAIL, "Missing devbox.json")


@register_common
def check_pre_commit(path: Path, report: VerificationReport) -> None:
    """Check for pre-commit configuration."""
    if (path / ".pre-commit-config.yaml").exists():
        report.add("Pre-commit configuration", Status.PASS)
    else:
        report.add("Pre-commit configuration", Status.FAIL, "Missing .pre-commit-config.yaml")


@register_common
def check_renovate(path: Path, report: VerificationReport) -> None:
    """Check for Renovate configuration."""
    if (path / "renovate.json").exists():
        report.add("Renovate configuration", Status.PASS)
    else:
        report.add("Renovate configuration", Status.WARN, "Missing renovate.json")


@register_common
def check_github_actions(path: Path, report: VerificationReport) -> None:
    """Check for GitHub Actions CI workflow."""
    if (path / ".github" / "workflows" / "ci.yaml").exists():
        report.add("GitHub Actions CI workflow", Status.PASS)
    else:
        report.add("GitHub Actions CI workflow", Status.FAIL, "Missing .github/workflows/ci.yaml")


@register_common
def check_editorconfig(path: Path, report: VerificationReport) -> None:
    """Check for EditorConfig settings."""
    editorconfig_path = path / ".editorconfig"

    status, message = check_file_exists(path, ".editorconfig")
    report.add("EditorConfig configuration", status, message)

    if status == Status.PASS:
        content = editorconfig_path.read_text()

        if "root = true" in content:
            report.add("EditorConfig has root=true", Status.PASS)
        else:
            report.add("EditorConfig has root=true", Status.WARN, "Missing root = true")

        if "end_of_line = lf" in content:
            report.add("EditorConfig line endings", Status.PASS)
        else:
            report.add("EditorConfig line endings", Status.WARN, "Missing end_of_line = lf")

        if "charset = utf-8" in content:
            report.add("EditorConfig charset", Status.PASS)
        else:
            report.add("EditorConfig charset", Status.WARN, "Missing charset = utf-8")

# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Common checks that apply to all projects."""

from __future__ import annotations

import json
import re

from typing import TYPE_CHECKING

from norms.checks import register_common
from norms.checks.base import check_file_exists
from norms.models import Status

if TYPE_CHECKING:
    from pathlib import Path

    from norms.models import VerificationReport


# -----------------------------------------------------------------------------
# Required Files
# -----------------------------------------------------------------------------


@register_common
def check_readme(path: Path, report: VerificationReport) -> None:
    """Check for README.md file."""
    status, message = check_file_exists(path / "README.md")
    report.add("README.md exists", status, message)


@register_common
def check_license(path: Path, report: VerificationReport) -> None:
    """Check for license file."""
    license_files = ["LICENSE", "LICENSE-APACHE-2.0", "LICENSE-MIT"]

    for lf in license_files:
        status, _ = check_file_exists(path / lf)

        if status == Status.PASS:
            report.add("LICENSE exists", status)
            return

    report.add("LICENSE exists", Status.FAIL, "Missing LICENSE file")


@register_common
def check_changelog(path: Path, report: VerificationReport) -> None:
    """Check for CHANGELOG.md file."""
    status, message = check_file_exists(path / "CHANGELOG.md", Status.WARN, "Consider adding CHANGELOG.md")
    report.add("CHANGELOG.md exists", status, message)


# -----------------------------------------------------------------------------
# Git Configuration
# -----------------------------------------------------------------------------


@register_common
def check_gitignore(path: Path, report: VerificationReport) -> None:
    """Check for .gitignore file."""
    status, message = check_file_exists(path / ".gitignore")
    report.add(".gitignore exists", status, message)


@register_common
def check_gitatributes(path: Path, report: VerificationReport) -> None:
    """Check for .gitattributes file with proper configuration."""
    gitattributes_path = path / ".gitattributes"

    status, message = check_file_exists(gitattributes_path)
    report.add(".gitattributes exists", status, message)

    if status != Status.PASS:
        return

    content = gitattributes_path.read_text()

    # Check for auto text normalization
    if re.search(r"^\*\s.+text=auto", content, re.MULTILINE):
        report.add(".gitattributes has text=auto", Status.PASS)
    else:
        report.add(
            ".gitattributes has text=auto", Status.WARN, "Consider adding '* text=auto' for line ending normalization"
        )

    # Check for LF line endings
    if re.search(r"^\*\s.+eol=lf", content, re.MULTILINE):
        report.add(".gitattributes enforces LF", Status.PASS)
    else:
        report.add(".gitattributes enforces LF", Status.WARN, "Consider adding '* eol=lf' for consistent line endings")

    # Check if some extensions are marked as binary
    if re.search(r"^\*\.([a-zA-Z0-9]+)\s+binary", content, re.MULTILINE):
        report.add(".gitattributes marks some files as binary", Status.PASS)
    else:
        report.add(
            ".gitattributes marks some files as binary", Status.WARN, "Consider marking binary file types as binary"
        )


# -----------------------------------------------------------------------------
# Editor Configuration
# -----------------------------------------------------------------------------


@register_common
def check_editorconfig(path: Path, report: VerificationReport) -> None:
    """Check for EditorConfig with proper settings."""
    editorconfig_path = path / ".editorconfig"

    status, message = check_file_exists(editorconfig_path)
    report.add(".editorconfig exists", status, message)

    if status != Status.PASS:
        return

    content = editorconfig_path.read_text()

    def check_setting(name: str, setting: str) -> None:
        """Check for a specific setting in the .editorconfig file."""
        if setting in content:
            report.add(f".editorconfig has {name}", Status.PASS)
        else:
            report.add(f".editorconfig has {name}", Status.WARN, f"Missing '{setting}'")

    check_setting("root=true", "root = true")
    check_setting("charset=utf-8", "charset = utf-8")
    check_setting("end_of_line=lf", "end_of_line = lf")
    check_setting("indent_style=space", "indent_style = space")
    check_setting("indent_size=4", "indent_size = 4")
    check_setting("insert_final_newline=true", "insert_final_newline = true")
    check_setting("trim_trailing_whitespace=true", "trim_trailing_whitespace = true")


@register_common
def check_devbox(path: Path, report: VerificationReport) -> None:
    """Check for Devbox configuration with proper settings."""
    devbox_path = path / "devbox.json"

    status, message = check_file_exists(devbox_path)
    report.add("devbox.json exists", status, message)

    if status != Status.PASS:
        return

    try:
        devbox_config = json.loads(devbox_path.read_text())
    except json.JSONDecodeError:
        report.add("devbox.json is valid JSON", Status.FAIL, "Invalid JSON in devbox.json")
        return

    report.add("devbox.json is valid JSON", Status.PASS)

    # Check for packages
    if devbox_config.get("packages"):
        report.add("devbox.json has packages defined", Status.PASS)
    else:
        report.add("devbox.json has packages defined", Status.WARN, "No packages defined in devbox.json")

    # Check for $schema
    if "$schema" in devbox_config:
        report.add("devbox.json has $schema", Status.PASS)
    else:
        report.add("devbox.json has $schema", Status.WARN, "Consider adding $schema for validation")

    # Check for devbox.lock
    lock_status, lock_message = check_file_exists(
        path / "devbox.lock", Status.WARN, "Consider adding devbox.lock for reproducibility"
    )
    report.add("devbox.lock exists", lock_status, lock_message)


# -----------------------------------------------------------------------------
# Pre-Commit
# -----------------------------------------------------------------------------


@register_common
def check_pre_commit(path: Path, report: VerificationReport) -> None:
    """Check for pre-commit configuration."""
    precommit_path = path / ".pre-commit-config.yaml"

    status, message = check_file_exists(precommit_path)
    report.add(".pre-commit-config.yaml exists", status, message)

    if status != Status.PASS:
        return


# -----------------------------------------------------------------------------
# Dependency Management
# -----------------------------------------------------------------------------


@register_common
def check_renovate(path: Path, report: VerificationReport) -> None:
    """Check for Renovate configuration with proper settings."""
    renovate_path = path / "renovate.json"

    status, message = check_file_exists(renovate_path, Status.WARN)
    report.add("Renovate configuration exists", status, message)

    if status != Status.PASS:
        return

    try:
        renovate_config = json.loads(renovate_path.read_text())
    except json.JSONDecodeError:
        report.add("renovate.json is valid JSON", Status.FAIL, "Invalid JSON")
        return

    report.add("renovate.json is valid JSON", Status.PASS)

    # Check for $schema
    if "$schema" in renovate_config:
        report.add("renovate.json has $schema", Status.PASS)
    else:
        report.add("renovate.json has $schema", Status.WARN, "Consider adding $schema for validation")

    # Check for extends
    extends = renovate_config.get("extends", [])
    if extends:
        report.add("Renovate has extends configured", Status.PASS)

        # Check for best-practices preset
        if any("best-practices" in preset for preset in extends):
            report.add("Renovate extends best-practices", Status.PASS)
        else:
            report.add("Renovate extends best-practices", Status.WARN, "Consider extending config:best-practices")
    else:
        report.add("Renovate has extends configured", Status.WARN, "Consider using extends for presets")


# -----------------------------------------------------------------------------
# Continuous Integration
# -----------------------------------------------------------------------------


@register_common
def check_github_actions(path: Path, report: VerificationReport) -> None:
    """Check for GitHub Actions CI workflow."""
    workflows_path = path / ".github" / "workflows"
    ci_path = workflows_path / "ci.yaml"

    if not workflows_path.exists():
        report.add("GitHub Actions workflows directory exists", Status.FAIL, "Missing .github/workflows/")
        return

    report.add("GitHub Actions workflows directory exists", Status.PASS)

    status, message = check_file_exists(ci_path, Status.FAIL, "Missing .github/workflows/ci.yaml")
    report.add("GitHub Actions CI workflow exists", status, message)

    if status != Status.PASS:
        return

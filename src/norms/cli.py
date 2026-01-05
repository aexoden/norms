# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2025 Jason Lynch <jason@aexoden.com>
"""Command-line interface for the norms tool."""

from __future__ import annotations

import json
import sys

from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Annotated, Any

from cyclopts import App, Parameter

app = App()


class Status(Enum):
    """Enumeration of check statuses."""

    PASS = "PASS"  # noqa: S105 ("PASS" refers to the test passing, not a password)
    FAIL = "FAIL"
    WARN = "WARN"
    SKIP = "SKIP"


class Language(Enum):
    """Enumeration of supported programming languages."""

    CPP = "cpp"
    PYTHON = "python"
    RUST = "rust"
    TYPESCRIPT = "typescript"


@dataclass
class CheckResult:
    """Result of a single check."""

    name: str
    status: Status
    message: str = ""


@dataclass
class VerificationReport:
    """Report of the verification process."""

    project_path: Path
    languages: list[Language]
    results: list[CheckResult] = field(default_factory=list[CheckResult])

    def add(self, name: str, status: Status, message: str = "") -> None:
        """Add a check result to the report.

        Args:
            name (str): The name of the check.
            status (Status): The status of the check.
            message (str): An optional message for the check.
        """
        self.results.append(CheckResult(name, status, message))

    @property
    def passed(self) -> int:
        """Get the number of passed checks."""
        return sum(1 for result in self.results if result.status == Status.PASS)

    @property
    def failed(self) -> int:
        """Get the number of failed checks."""
        return sum(1 for result in self.results if result.status == Status.FAIL)

    @property
    def warnings(self) -> int:
        """Get the number of warnings."""
        return sum(1 for result in self.results if result.status == Status.WARN)

    def print_report(self) -> None:
        """Print the verification report to the console."""
        print(f"{'=' * 60}")
        print("Project Standards Verification Report")
        print(f"{'=' * 60}")
        print(f"Path: {self.project_path.resolve()}")
        print(f"Languages: {', '.join(language.value for language in self.languages) or 'None'}")
        print(f"{'=' * 60}\n")

        # Group by status
        for status in [Status.FAIL, Status.WARN, Status.PASS, Status.SKIP]:
            items = [result for result in self.results if result.status == status]
            if not items:
                continue

            status_label = {
                Status.PASS: "Passed",
                Status.FAIL: "Failed",
                Status.WARN: "Warnings",
                Status.SKIP: "Skipped",
            }[status]

            print(f"{status_label}:")

            for item in items:
                msg = f" - {item.message}" if item.message else ""
                print(f"  [{status.value}] {item.name}{msg}")

            print()

        print(f"{'=' * 60}")
        print(f"\nSummary: {self.passed} passed, {self.failed} failed, {self.warnings} warnings")
        print(f"{'=' * 60}")


def check_file_exists(path: Path, filename: str) -> tuple[Status, str]:
    """Check if a file exists in the given path.

    Args:
        path (Path): The path to check.
        filename (str): The name of the file to check for.

    Returns:
        tuple[Status, str]: The status and message of the check.
    """
    if (path / filename).exists():
        return (Status.PASS, "")

    return (Status.FAIL, f"Missing {filename}")


def check_common(path: Path, report: VerificationReport) -> None:
    """Run checks common to all projects."""
    # Check for required configuration files
    required_files = [
        (".editorconfig", "EditorConfig configuration"),
        (".gitattributes", "Git attributes"),
        (".gitignore", "Git ignore"),
        ("LICENSE", "License file"),
        ("README.md", "README"),
    ]

    for filename, description in required_files:
        status, message = check_file_exists(path, filename)
        report.add(description, status, message)

    # Check for Devbox configuration
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

    # Pre-commit
    if (path / ".pre-commit-config.yaml").exists():
        report.add("Pre-commit configuration", Status.PASS)
    else:
        report.add("Pre-commit configuration", Status.FAIL, "Missing .pre-commit-config.yaml")

    # Renovate
    if (path / "renovate.json").exists():
        report.add("Renovate configuration", Status.PASS)
    else:
        report.add("Renovate configuration", Status.WARN, "Missing renovate.json")


def check_cpp(path: Path, report: VerificationReport) -> None:
    """Run checks specific to C++ projects."""


def check_python(path: Path, report: VerificationReport) -> None:
    """Run checks specific to Python projects."""


def check_rust(path: Path, report: VerificationReport) -> None:
    """Run checks specific to Rust projects."""


def check_typescript(path: Path, report: VerificationReport) -> None:
    """Run checks specific to TypeScript projects."""


def detect_languages(path: Path) -> list[Language]:
    """Detect the programming languages used in the project at the given path.

    Args:
        path (Path): The path to the project.

    Returns:
        list[Language]: The list of detected programming languages.
    """
    languages: list[Language] = []

    if (path / "CMakeLists.txt").exists():
        languages.append(Language.CPP)

    if (path / "pyproject.toml").exists():
        languages.append(Language.PYTHON)

    if (path / "Cargo.toml").exists():
        languages.append(Language.RUST)

    if (path / "package.json").exists():
        languages.append(Language.TYPESCRIPT)

    return languages


def verify_project(path: Path) -> VerificationReport:
    """Verify the project at the given path.

    Args:
        path (Path): The path to the project to verify.

    Returns:
        VerificationReport: The report of the verification.
    """
    if not path.is_dir():
        print(f"Error: The path '{path}' is not a valid directory.", file=sys.stderr)
        sys.exit(1)

    languages = detect_languages(path)
    report = VerificationReport(path, languages)

    # Run common checks
    check_common(path, report)

    # Run language-specific checks
    if len(languages) == 0:
        report.add("Language detection", Status.WARN, "No supported programming languages detected")

    for language in languages:
        if language == Language.CPP:
            check_cpp(path, report)
        elif language == Language.PYTHON:
            check_python(path, report)
        elif language == Language.RUST:
            check_rust(path, report)
        elif language == Language.TYPESCRIPT:
            check_typescript(path, report)

    return report


@app.command
def verify(path: Path, *, output_json: Annotated[bool, Parameter(name=["--json"])] = False) -> None:
    """Verify the project at the given path is correctly configured.

    Args:
        path (Path): The path to the project to verify.
        output_json (bool): Whether to output the verification results in JSON format.
    """
    report = verify_project(path)

    if output_json:
        output: dict[str, Any] = {
            "path": str(report.project_path.resolve()),
            "languages": [language.value for language in report.languages],
            "summary": {
                "passed": report.passed,
                "failed": report.failed,
                "warnings": report.warnings,
            },
            "results": [
                {
                    "name": result.name,
                    "status": result.status.name.lower(),
                    "message": result.message,
                }
                for result in report.results
            ],
        }
        print(json.dumps(output, indent=2))
    else:
        report.print_report()

    sys.exit(1 if report.failed > 0 else 0)


def run() -> None:
    """Run the command-line interface."""
    app()

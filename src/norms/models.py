# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Data models for the norms tool."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable
    from pathlib import Path


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
    languages: Iterable[Language]
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

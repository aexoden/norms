# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Project verification orchestration."""

from __future__ import annotations

import sys

from typing import TYPE_CHECKING

from norms.checks import run_all_checks
from norms.detection import detect_languages
from norms.models import Status, VerificationReport

if TYPE_CHECKING:
    from pathlib import Path


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

    # Run all registered checks
    run_all_checks(path, languages, report)

    # Warn if no languages detected
    if len(languages) == 0:
        report.add("Language detection", Status.WARN, "No supported programming languages detected")

    return report

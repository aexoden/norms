# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Check registry and runner for the norms tool."""

from __future__ import annotations

from typing import TYPE_CHECKING

from norms.models import Language

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable
    from pathlib import Path

    from norms.models import VerificationReport

    CheckFunction = Callable[[Path, VerificationReport], None]

# Registries for checks
_common_checks: list[CheckFunction] = []
_language_checks: dict[Language, list[CheckFunction]] = {lang: [] for lang in Language}


def register_common(fn: CheckFunction) -> CheckFunction:
    """Register a common check that runs for all projects.

    Args:
        fn (CheckFunction): The check function to register.

    Returns:
        CheckFunction: The registered unmodified check function.
    """
    _common_checks.append(fn)
    return fn


def register_language(language: Language) -> Callable[[CheckFunction], CheckFunction]:
    """Decorator to register a language-specific check.

    Args:
        language (Language): The programming language for which the check is applicable.

    Returns:
        Callable[[CheckFunction], CheckFunction]: A decorator that registers the check function.
    """

    def decorator(fn: CheckFunction) -> CheckFunction:
        _language_checks[language].append(fn)
        return fn

    return decorator


def run_all_checks(path: Path, languages: Iterable[Language], report: VerificationReport) -> None:
    """Run all registered checks for the given project path and languages.

    Args:
        path (Path): The path to the project.
        languages (Iterable[Language]): The programming languages used in the project.
        report (VerificationReport): The report to which check results will be added.
    """
    for check in _common_checks:
        check(path, report)

    for language in languages:
        for check in _language_checks.get(language, []):
            check(path, report)

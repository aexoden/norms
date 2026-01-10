# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Language detection utilities for the norms tool."""

from __future__ import annotations

from typing import TYPE_CHECKING

from norms.models import Language

if TYPE_CHECKING:
    from pathlib import Path


def detect_languages(path: Path) -> set[Language]:
    """Detect the programming languages used in the project at the given path.

    Args:
        path (Path): The path to the project.

    Returns:
        list[Language]: The list of detected programming languages.
    """
    languages: set[Language] = set()

    if (path / "CMakeLists.txt").exists():
        languages.add(Language.CPP)

    if (path / "pyproject.toml").exists():
        languages.add(Language.PYTHON)

    if (path / "Cargo.toml").exists():
        languages.add(Language.RUST)

    if (path / "package.json").exists():
        languages.add(Language.TYPESCRIPT)

    return languages

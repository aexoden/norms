# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Base utilities for check functions."""

from __future__ import annotations

from typing import TYPE_CHECKING

from norms.models import Status

if TYPE_CHECKING:
    from pathlib import Path


def check_file_exists(path: Path, filename: str) -> tuple[Status, str]:
    """Check if a file exists in the given path.

    Args:
        path (Path): The path to check.
        filename (str): The name of the file to check for.

    Returns:
        tuple[Status, str]: A tuple containing the status and message of the check.
    """
    if (path / filename).exists():
        return (Status.PASS, "")

    return (Status.FAIL, f"Missing {filename}")

# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Base utilities for check functions."""

from __future__ import annotations

from typing import TYPE_CHECKING

from norms.models import Status

if TYPE_CHECKING:
    from pathlib import Path


def check_file_exists(
    path: Path, fail_status: Status = Status.FAIL, fail_message: str | None = None
) -> tuple[Status, str]:
    """Check if a file exists at the given path.

    Args:
        path (Path): The full path to the file to check.
        fail_status (Status): The status to return if the file does not exist.
        fail_message (str): The message to return if the file does not exist.

    Returns:
        tuple[Status, str]: A tuple containing the status and message of the check.
    """
    if path.exists():
        return (Status.PASS, "")

    if fail_message is None:
        fail_message = f"Missing {path.name}"

    return (fail_status, fail_message)

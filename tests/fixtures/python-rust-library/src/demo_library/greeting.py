# SPDX-License-Identifier: MIT OR Apache-2.0
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Greetings."""

from __future__ import annotations


def greeting(name: str) -> str:
    """Build a greeting.

    Args:
        name: The name to greet.

    Returns:
        str: The greeting.
    """
    return f"Hello, {name}!"

# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Tests for the greeting module."""

from __future__ import annotations

from demo_python.greeting import greeting


def test_greets_by_name() -> None:
    """The greeting includes the name."""
    assert greeting("world") == "Hello, world!"

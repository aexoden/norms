# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Command-line entry point."""

from __future__ import annotations

import sys

from demo_python.greeting import greeting


def main() -> None:
    """Print a greeting."""
    sys.stdout.write(greeting("world") + "\n")

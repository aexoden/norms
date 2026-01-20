# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Check registry and runner for the norms tool."""

from norms.checks.registry import register_common, register_language, run_all_checks

__all__ = ["register_common", "register_language", "run_all_checks"]

# Import check modules to register their checks
from norms.checks import (  # noqa: F401 (importing triggers registration)
    common,  # pyright: ignore[reportUnusedImport]
    python,  # pyright: ignore[reportUnusedImport]
)

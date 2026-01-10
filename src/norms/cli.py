# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
"""Command-line interface for the norms tool."""

from __future__ import annotations

import json
import sys

from pathlib import Path
from typing import Annotated, Any

from cyclopts import App, Parameter

from norms.verification import verify_project

app = App()


@app.command
def verify(path: Path, *, output_json: Annotated[bool, Parameter(name=["--json"])] = False) -> None:
    """Verify the project at the given path is correctly configured.

    Args:
        path (Path): The path to the project to verify.
        output_json (bool): Whether to output the verification results in JSON format.
    """
    report = verify_project(path)

    if output_json:
        output: dict[str, Any] = {
            "path": str(report.project_path.resolve()),
            "languages": [language.value for language in report.languages],
            "summary": {
                "passed": report.passed,
                "failed": report.failed,
                "warnings": report.warnings,
            },
            "results": [
                {
                    "name": result.name,
                    "status": result.status.name.lower(),
                    "message": result.message,
                }
                for result in report.results
            ],
        }
        print(json.dumps(output, indent=2))
    else:
        report.print_report()

    sys.exit(1 if report.failed > 0 else 0)


def run() -> None:
    """Run the command-line interface."""
    app()

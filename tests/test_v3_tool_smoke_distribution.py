"""Installed-distribution smoke contract for the v3 MIME-pruned product."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SMOKE_TOOL = PROJECT_ROOT / "tools" / "smoke_distribution.py"


def test_installed_distribution_smoke_exercises_mime_pruned_output() -> None:
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.startswith("COVERAGE_")
    }
    result = subprocess.run(
        [sys.executable, "-B", str(SMOKE_TOOL)],
        check=False,
        cwd=PROJECT_ROOT,
        env=environment,
        text=True,
        capture_output=True,
    )
    assert result.returncode == 0, result.stderr

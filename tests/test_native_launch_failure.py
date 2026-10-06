"""Oversized app launches get selection guidance at the real OS boundary."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

UI = Path(__file__).resolve().parents[1] / "integrations/macos-ui"


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS process launch")
def test_e2big_is_reported_as_selection_size_not_missing_python(tmp_path: Path) -> None:
    app = tmp_path / "LaunchChecks.app"
    executable = app / "Contents/MacOS/launch-checks"
    executable.parent.mkdir(parents=True)
    (app / "Contents/Resources").mkdir()
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-O",
            str(UI / "App/LaunchExplanation.swift"),
            str(UI / "App/RequestTransport.swift"),
            str(UI / "App/ProcessingRun.swift"),
            str(UI / "App/ProgressStream.swift"),
            str(UI / "App/RuntimeConfiguration.swift"),
            str(UI / "App/UITrace.swift"),
            str(UI / "ReportModel.swift"),
            str(UI / "LauncherFailure.swift"),
            str(UI / "test-launch.swift"),
            "-o",
            str(executable),
        ],
        check=True,
        timeout=120,
    )
    result = subprocess.run(
        [str(executable)],
        env={**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert "Native launch failure checks passed." in result.stdout

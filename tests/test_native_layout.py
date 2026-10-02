"""Exercise native table resizing and result readability with production views."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

UI = Path(__file__).resolve().parents[1] / "integrations/macos-ui"


@pytest.mark.skipif(sys.platform != "darwin", reason="AppKit layout")
def test_native_batch_results_remain_readable_when_resizing(tmp_path: Path) -> None:
    executable = tmp_path / "layout-tests"
    sources = [UI / "Artwork.swift", UI / "ReportModel.swift", UI / "test-layout.swift"]
    sources.extend(
        path
        for path in sorted((UI / "App").glob("*.swift"))
        if path.name != "ApplicationEntry.swift"
    )
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-O",
            "-warnings-as-errors",
            "-parse-as-library",
            *map(str, sources),
            "-o",
            str(executable),
        ],
        env={**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        check=True,
        timeout=180,
    )
    subprocess.run([str(executable)], check=True, timeout=30)

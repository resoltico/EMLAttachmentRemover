"""Exercise the actual AppKit palette and appearance-change notification boundaries."""

from __future__ import annotations

import os
import platform
import subprocess
import sys
from pathlib import Path

import pytest

UI = Path(__file__).resolve().parents[1] / "integrations/macos-ui"


@pytest.mark.skipif(sys.platform != "darwin", reason="AppKit artwork behavior")
@pytest.mark.parametrize("optimization", ["-Onone", "-O"])
def test_native_artwork_palette_and_appearance_changes(
    tmp_path: Path, optimization: str
) -> None:
    """Both production and debug builds retain contrast and dynamic invalidation."""
    executable = tmp_path / "artwork-tests"
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            optimization,
            "-target",
            "arm64-apple-macosx14.0"
            if platform.machine() == "arm64"
            else "x86_64-apple-macosx14.0",
            str(UI / "Artwork.swift"),
            str(UI / "test-artwork.swift"),
            "-o",
            str(executable),
        ],
        env={**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        check=True,
        timeout=180,
    )
    subprocess.run([str(executable)], check=True, timeout=30)

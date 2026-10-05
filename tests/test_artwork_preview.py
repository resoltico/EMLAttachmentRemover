"""Compile the artwork review tool and inspect its exported color contract."""

from __future__ import annotations

import os
import struct
import subprocess
import sys
from pathlib import Path

import pytest

UI = Path(__file__).resolve().parents[1] / "integrations/macos-ui"


@pytest.mark.skipif(sys.platform != "darwin", reason="AppKit artwork review")
def test_artwork_preview_compiles_and_exports_srgb(tmp_path: Path) -> None:
    executable = tmp_path / "artwork-preview"
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-parse-as-library",
            str(UI / "Artwork.swift"),
            str(UI / "ArtworkPreview.swift"),
            "-o",
            str(executable),
        ],
        env={**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        check=True,
        timeout=180,
    )
    destination = tmp_path / "review"
    subprocess.run([str(executable), str(destination)], check=True, timeout=30)
    for kind in ("identity", "processing", "success", "attention", "stopped"):
        for size in (16, 28, 32, 128, 256):
            _assert_srgb_png(destination / f"{kind}-{size}.png", (size, size))
    _assert_srgb_png(destination / "identity-source.png", (512, 512))
    _assert_srgb_png(destination / "original-artwork-review.png", (1800, 1300))
    assert len(list(destination.iterdir())) == 27


def _assert_srgb_png(path: Path, dimensions: tuple[int, int]) -> None:
    data = path.read_bytes()
    assert data.startswith(b"\x89PNG\r\n\x1a\n")
    assert struct.unpack(">II", data[16:24]) == dimensions
    offset = 8
    color_profile = None
    while offset < len(data):
        size = int.from_bytes(data[offset : offset + 4])
        assert offset + size + 12 <= len(data)
        if data[offset + 4 : offset + 8] == b"sRGB":
            color_profile = data[offset + 8 : offset + 8 + size]
        offset += size + 12
    assert offset == len(data)
    if color_profile is not None:
        assert color_profile in {b"\x00", b"\x01", b"\x02", b"\x03"}
    else:
        profile = subprocess.check_output(
            ["/usr/bin/sips", "-g", "profile", str(path)],
            text=True,
            env={**os.environ, "LC_ALL": "C"},
            timeout=10,
        )
        assert "profile: sRGB" in profile

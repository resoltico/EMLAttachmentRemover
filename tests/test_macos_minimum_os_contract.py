"""Keep the native macOS 15 minimum deployment target consistent across outputs."""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "integrations/macos-ui"


def test_native_deployment_floor_is_macos15_for_both_cpus() -> None:
    """No qualified native artifact may silently target the retired macOS floor."""
    build = (UI / "build.sh").read_text(encoding="utf-8")
    icon = (UI / "compile-icon.sh").read_text(encoding="utf-8")
    model = (UI / "test.sh").read_text(encoding="utf-8")
    archive = (ROOT / "tools/macos_archive.py").read_text(encoding="utf-8")
    assert "'LSMinimumSystemVersion': '15.0'" in build
    assert '-target "$ARCH-apple-macosx15.0"' in build
    assert "--minimum-deployment-target 15.0 --platform macosx" in icon
    assert "arm64:15.*)" in model
    assert "x86_64-apple-macosx15.0" in model
    assert '"LSMinimumSystemVersion": "15.0"' in archive
    for source in (build, icon, model):
        assert "macosx14.0" not in source
        assert "minimum-deployment-target 14.0" not in source

"""Require portable line endings for repository text and public scripts."""

from __future__ import annotations

from pathlib import Path
from typing import Final

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
ATTRIBUTES: Final = PROJECT_ROOT / ".gitattributes"
SHELL_ROOT: Final = PROJECT_ROOT / "integrations" / "macos-shortcuts"


def test_repository_text_and_shell_scripts_are_declared_lf_only() -> None:
    """Prevent platform checkout rules from changing public text bytes."""
    assert ATTRIBUTES.read_bytes() == (b"* text=auto eol=lf\n*.sh text eol=lf\n")
    scripts = tuple(sorted(SHELL_ROOT.glob("*.sh")))
    assert scripts
    assert all(b"\r" not in script.read_bytes() for script in scripts)

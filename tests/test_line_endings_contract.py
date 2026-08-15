"""Require portable line endings for public shell scripts."""

from __future__ import annotations

from pathlib import Path
from typing import Final

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
ATTRIBUTES: Final = PROJECT_ROOT / ".gitattributes"
SHELL_ROOT: Final = PROJECT_ROOT / "integrations" / "macos-shortcuts"


def test_shell_scripts_are_declared_lf_only() -> None:
    """Prevent Git's Windows checkout from corrupting POSIX shell scripts."""
    assert ATTRIBUTES.read_bytes() == b"*.sh text eol=lf\n"
    scripts = tuple(sorted(SHELL_ROOT.glob("*.sh")))
    assert scripts
    assert all(b"\r" not in script.read_bytes() for script in scripts)

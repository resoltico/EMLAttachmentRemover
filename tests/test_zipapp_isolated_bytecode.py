"""Isolated verification disables bytecode independently of environment settings."""

from __future__ import annotations

from typing import TYPE_CHECKING

from tools import build_zipapp

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_zipapp_verification_disables_bytecode_inside_the_real_interpreter(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The executable guard fails unless isolated verification actually sets -B."""
    members = (("__main__.py", b"import sys\nassert sys.dont_write_bytecode\n"),)
    monkeypatch.setattr(build_zipapp, "_archive_members", lambda _metadata: members)
    monkeypatch.setenv("PYTHONDONTWRITEBYTECODE", "1")
    result = build_zipapp.build_zipapp(tmp_path / "guard.pyz", verify=True)
    assert result.is_file()

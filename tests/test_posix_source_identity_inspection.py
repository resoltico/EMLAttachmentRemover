"""Identity-only POSIX inventory does not depend on final-address availability."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_posix

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX native descriptor boundary")
def test_identity_only_inspection_never_queries_a_final_address(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(b"retained")

    def unavailable(_descriptor: int) -> None:
        pytest.fail("identity-only inspection queried the final source address")

    monkeypatch.setattr(native_posix, "_final_address", unavailable)
    identity = native_posix.inspect_source_identity(str(source))
    assert identity.inode == source.stat().st_ino
    assert identity.is_regular()

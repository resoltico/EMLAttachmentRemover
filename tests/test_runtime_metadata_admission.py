"""Authenticated runtime metadata has bounded sizes and complete required notices."""

from __future__ import annotations

import hashlib
import io
import tarfile
from typing import TYPE_CHECKING

import pytest
from tools import macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture

if TYPE_CHECKING:
    from pathlib import Path
    from typing import IO


@pytest.mark.parametrize(
    "member_kind",
    ["directory", "empty", "oversized", "missing-notice", "at-limit", "single-value"],
)
def test_runtime_metadata_limits_and_required_notices(
    tmp_path: Path, member_kind: str
) -> None:
    """Accept complete metadata at the limit and refuse invalid source declarations."""
    original, pin = runtime_archive_fixture(tmp_path)
    archive = tmp_path / "changed.tar.zst"
    with (
        tarfile.open(original, "r:zst") as source,
        tarfile.open(archive, "w:zst") as output,
    ):
        for member in source.getmembers():
            if member_kind == "missing-notice" and member.name.startswith(
                "python/licenses/"
            ):
                continue
            stream = source.extractfile(member)
            stream = _metadata_variant(member, stream, member_kind)
            output.addfile(member, stream)
            if stream is not None:
                stream.close()
    selected = pin.copy()
    selected["size"] = archive.stat().st_size
    selected["sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
    if member_kind == "at-limit":
        root = macos_runtime_source.extract(archive, tmp_path / "extracted", selected)
        assert (
            root / "PYTHON.json"
        ).stat().st_size == macos_runtime_source.MAX_METADATA
        assert (
            root / "licenses/LICENSE.cpython.txt"
        ).read_bytes() == b"original notice"
    else:
        message = {
            "missing-notice": "runtime installation or original notices are missing",
            "single-value": (
                "upstream runtime metadata differs from the required contract"
            ),
        }.get(member_kind, "invalid upstream runtime metadata member")
        with pytest.raises(ValueError, match="^" + message + "$"):
            macos_runtime_source.extract(archive, tmp_path / "extracted", selected)


def _metadata_variant(
    member: tarfile.TarInfo, stream: IO[bytes] | None, kind: str
) -> IO[bytes] | None:
    if member.name != "python/PYTHON.json" or kind == "missing-notice":
        return stream
    original = stream.read() if stream is not None and kind == "at-limit" else b""
    if stream is not None:
        stream.close()
    content = (
        b" " * (macos_runtime_source.MAX_METADATA + 1) if kind == "oversized" else b""
    )
    if kind == "at-limit":
        content = original.ljust(macos_runtime_source.MAX_METADATA, b" ")
    elif kind == "single-value":
        content = b"0"
    member.size = len(content)
    if kind == "directory":
        member.type = tarfile.DIRTYPE
        return None
    return io.BytesIO(content)

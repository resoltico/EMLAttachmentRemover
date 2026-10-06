"""Runtime archive authentication and downloads use bounded cumulative reads."""

from __future__ import annotations

import hashlib
import io
import urllib.request
from typing import TYPE_CHECKING
from unittest.mock import MagicMock

import pytest
from tools import macos_runtime_source

if TYPE_CHECKING:
    from pathlib import Path


class BoundedStream(io.BytesIO):
    """Refuse unbounded reads from the archive or upstream response fixture."""

    def read(self, size: int | None = -1) -> bytes:
        assert size == 3
        return super().read(size)


def _pin(data: bytes) -> macos_runtime_source.RuntimePin:
    """Describe independently supplied fixture bytes.

    Returns:
        Their expected size, digest and declared origin.

    """
    return {
        "url": "https://github.com/astral-sh/python-build-standalone/releases/download/public/example.tar.zst",
        "sha256": hashlib.sha256(data).hexdigest(),
        "size": len(data),
        "target": "aarch64-apple-darwin",
    }


def test_authentication_streams_the_archive_in_bounded_chunks(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    data = b"public archive fixture"
    archive = MagicMock()
    archive.is_symlink.return_value = False
    archive.stat.return_value.st_size = len(data)
    archive.open.return_value.__enter__.return_value = BoundedStream(data)
    monkeypatch.setattr(macos_runtime_source, "CHUNK", 3)
    macos_runtime_source.verify(archive, _pin(data))
    archive.open.assert_called_once_with("rb")
    archive.open.return_value.__exit__.assert_called_once()


@pytest.mark.parametrize("overflow", [False, True])
def test_download_accounts_for_every_bounded_response_chunk(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    overflow: bool,
) -> None:
    data = b"public archive fixture"
    received = data + b"extra bytes" if overflow else data
    monkeypatch.setattr(macos_runtime_source, "CHUNK", 3)

    def response(url: str, *, timeout: int) -> BoundedStream:
        assert url == _pin(data)["url"]
        assert timeout == 30
        return BoundedStream(received)

    monkeypatch.setattr(urllib.request, "urlopen", response)
    target = tmp_path / "download"
    if overflow:
        with pytest.raises(
            ValueError, match=r"^runtime download exceeds pinned byte count$"
        ):
            macos_runtime_source.download(target, _pin(data))
        assert target.stat().st_size <= len(data)
        assert received.startswith(target.read_bytes())
    else:
        macos_runtime_source.download(target, _pin(data))
        assert target.read_bytes() == data

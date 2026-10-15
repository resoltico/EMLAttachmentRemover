"""Tool archive transfers retain authentication and a disk byte ceiling."""

from __future__ import annotations

import hashlib
import io
from typing import TYPE_CHECKING

import pytest
from tools import pinned_download

if TYPE_CHECKING:
    from pathlib import Path


def test_archive_is_copied_exactly_and_without_overwriting(tmp_path: Path) -> None:
    contents = b"authenticated archive"
    target = tmp_path / "archive"
    digest = hashlib.sha256(contents).hexdigest()
    pinned_download.copy_archive(
        io.BytesIO(contents), target, size=len(contents), sha256=digest
    )
    assert target.read_bytes() == contents
    with pytest.raises(FileExistsError):
        pinned_download.copy_archive(
            io.BytesIO(b"different"), target, size=len(contents), sha256=digest
        )
    assert target.read_bytes() == contents


@pytest.mark.parametrize("contents", [b"short", b"tampered archive"])
def test_archive_size_and_digest_are_both_authenticated(
    tmp_path: Path, contents: bytes
) -> None:
    expected = b"correct archive!"
    assert len(expected) == len(b"tampered archive")
    with pytest.raises(ValueError, match="size or checksum differs"):
        pinned_download.copy_archive(
            io.BytesIO(contents),
            tmp_path / "archive",
            size=len(expected),
            sha256=hashlib.sha256(expected).hexdigest(),
        )


def test_oversized_transfer_stops_before_writing_excess_bytes(tmp_path: Path) -> None:
    limit = pinned_download.CHUNK + 1
    target = tmp_path / "archive"
    source = io.BytesIO(b"x" * (3 * pinned_download.CHUNK))
    with pytest.raises(ValueError, match="exceeds pinned byte count"):
        pinned_download.copy_archive(source, target, size=limit, sha256="unused")
    assert target.stat().st_size == pinned_download.CHUNK
    assert source.tell() == 2 * pinned_download.CHUNK


@pytest.mark.parametrize(
    "contents", [b"", b"a" * (pinned_download.CHUNK + 17)], ids=("empty", "multichunk")
)
def test_empty_and_multichunk_archives_preserve_exact_bytes(
    tmp_path: Path, contents: bytes
) -> None:
    target = tmp_path / "archive"
    pinned_download.copy_archive(
        io.BytesIO(contents),
        target,
        size=len(contents),
        sha256=hashlib.sha256(contents).hexdigest(),
    )
    assert target.read_bytes() == contents


def test_matching_digest_cannot_excuse_a_short_archive(tmp_path: Path) -> None:
    contents = b"complete received stream"
    with pytest.raises(
        ValueError, match=r"^tool download size or checksum differs from pin$"
    ):
        pinned_download.copy_archive(
            io.BytesIO(contents),
            tmp_path / "archive",
            size=len(contents) + 1,
            sha256=hashlib.sha256(contents).hexdigest(),
        )


def test_single_excess_byte_is_rejected_before_that_block_is_written(
    tmp_path: Path,
) -> None:
    target = tmp_path / "archive"
    with pytest.raises(ValueError, match=r"^tool download exceeds pinned byte count$"):
        pinned_download.copy_archive(
            io.BytesIO(b"ab"),
            target,
            size=1,
            sha256=hashlib.sha256(b"a").hexdigest(),
        )
    assert target.read_bytes() == b""


def test_transfer_read_failure_is_propagated_with_only_received_bytes(
    tmp_path: Path,
) -> None:
    class InterruptedSource(io.BytesIO):
        def read(self, size: int | None = -1) -> bytes:
            if self.tell():
                message = "interrupted transfer"
                raise OSError(message)
            return super().read(size)

    target = tmp_path / "archive"
    with pytest.raises(OSError, match=r"^interrupted transfer$"):
        pinned_download.copy_archive(
            InterruptedSource(b"prefix"),
            target,
            size=6,
            sha256=hashlib.sha256(b"prefix").hexdigest(),
        )
    assert target.read_bytes() == b"prefix"

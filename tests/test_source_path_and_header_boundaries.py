"""Real-boundary regressions for source spelling and complete header deletion."""

from __future__ import annotations

import os
import subprocess
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

import pytest

from eml_attachment_remover import batch_inventory
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import ExitCode, ItemStatus
from eml_attachment_remover.native_paths import inspect_source


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory symlink spellings")
def test_output_directory_reruns_share_one_kernel_source_address(
    tmp_path: Path,
) -> None:
    source = tmp_path / "inbox" / "basic.eml"
    source.parent.mkdir()
    source.write_bytes(b"From: a@example.test\r\n\r\nbody\r\n")
    alias = tmp_path / "alias"
    alias.symlink_to(source.parent, target_is_directory=True)
    child = tmp_path / "child"
    child.mkdir()
    output = tmp_path / "out"
    output.mkdir()
    options = BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=str(output),
    )
    spellings = [
        str(source),
        str(child / ".." / "inbox" / source.name),
        str(alias / source.name),
    ]
    destinations = []
    for index, spelling in enumerate(spellings):
        item = execute([spelling], options).items[0]
        assert item.status is (
            ItemStatus.CREATED if index == 0 else ItemStatus.EXISTING_VERIFIED
        )
        assert item.destination is not None
        destinations.append(item.destination.request.text)
    assert len(set(destinations)) == 1
    assert len(list(output.glob("*.eml"))) == 1


def test_unavailable_kernel_address_does_not_fall_back_to_path_spelling(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Subject: test\r\n\r\nbody\r\n")
    identity, _address = inspect_source(str(source))
    monkeypatch.setattr(
        batch_inventory, "inspect_source", lambda _source: (identity, None)
    )
    output = tmp_path / "out"
    output.mkdir()
    item = execute(
        [str(source)],
        BatchOptions(
            dry_run=True,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=str(output),
        ),
    ).items[0]
    assert item.error is not None
    assert item.error.code is ExitCode.INPUT_ERROR
    assert not list(output.iterdir())


@pytest.mark.skipif(os.name == "nt", reason="POSIX literal tilde filename")
def test_cli_opens_literal_tilde_argument_instead_of_home_directory(
    tmp_path: Path,
) -> None:
    folder = tmp_path / "~"
    folder.mkdir()
    (folder / "message.eml").write_bytes(b"Subject: literal\r\n\r\nbody\r\n")
    result = subprocess.run(
        [sys.executable, "-B", "-m", "eml_attachment_remover", "--", "~/message.eml"],
        cwd=tmp_path,
        env={**os.environ, "HOME": str(tmp_path / "another-home")},
        capture_output=True,
        check=False,
        text=True,
        timeout=10,
    )
    assert result.returncode == 0, result.stderr
    assert (folder / "message.mime-pruned.eml").is_file()


@pytest.mark.parametrize("newline", [b"\r\n", b"\n", b"\r"])
@pytest.mark.parametrize("folded", [False, True])
def test_removing_last_header_keeps_exact_header_body_separator(
    tmp_path: Path, newline: bytes, *, folded: bool
) -> None:
    stale = b"Content-Length: 999" + (newline + b" 999" if folded else b"")
    raw = newline.join([
        b"From: a@example.test",
        b"MIME-Version: 1.0",
        b'Content-Type: multipart/mixed; boundary="B"',
        stale,
        b"",
        b"--B",
        b"Content-Type: text/plain",
        b"",
        b"body",
        b"--B",
        b"Content-Type: application/octet-stream",
        b"Content-Disposition: attachment",
        b"",
        b"bytes",
        b"--B--",
        b"",
    ])
    source = tmp_path / "message.eml"
    source.write_bytes(raw)
    item = execute(
        [str(source)],
        BatchOptions(
            dry_run=False,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
    ).items[0]
    assert item.status is ItemStatus.CREATED
    candidate = source.with_name("message.mime-pruned.eml").read_bytes()
    expected_prefix = newline.join([
        b"From: a@example.test",
        b"MIME-Version: 1.0",
        b'Content-Type: multipart/mixed; boundary="B"',
        b"",
        b"--B",
    ])
    assert candidate.startswith(expected_prefix)
    assert b"Content-Length:" not in candidate
    assert b"Content-Disposition: attachment" not in candidate


@pytest.mark.skipif(os.name == "nt", reason="POSIX source diagnostic formatting")
def test_missing_source_diagnostic_keeps_requested_directory(tmp_path: Path) -> None:
    source = tmp_path / "inbox" / "missing.eml"
    source.parent.mkdir()
    item = execute(
        [str(source)],
        BatchOptions(
            dry_run=True,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
    ).items[0]
    assert item.error is not None
    assert str(source) in item.error.message
    assert "b'missing.eml'" not in item.error.message

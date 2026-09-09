"""Wire and source-binding preservation regressions for v3 MIME pruning."""

from __future__ import annotations

from typing import TYPE_CHECKING

from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import BatchLedger, ItemStatus

if TYPE_CHECKING:
    from pathlib import Path


def _options() -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _run(
    tmp_path: Path, raw: bytes, *, name: str = "message.eml"
) -> tuple[Path, BatchLedger]:
    source = tmp_path / name
    source.write_bytes(raw)
    return source, execute([str(source)], _options())


def test_symlink_dotdot_source_uses_kernel_selected_object(tmp_path: Path) -> None:
    left = tmp_path / "left"
    right = tmp_path / "right"
    (right / "sub").mkdir(parents=True)
    left.mkdir()
    (left / "source.eml").write_bytes(b"Content-Type: text/plain\r\n\r\nwrong\r\n")
    (right / "source.eml").write_bytes(b"Content-Type: text/plain\r\n\r\nright\r\n")
    (left / "link").symlink_to(right / "sub", target_is_directory=True)
    requested = left / "link" / ".." / "source.eml"
    ledger = execute([str(requested)], _options())
    output = right / "source.mime-pruned.eml"
    assert ledger.items[0].status is ItemStatus.CREATED
    assert b"right" in output.read_bytes()
    assert not (left / "source.mime-pruned.eml").exists()


def test_quoted_printable_and_binary_retained_payloads_are_not_regenerated(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/alternative; boundary=q\n\n"
        b"--q\nContent-Type: text/plain\n"
        b"Content-Transfer-Encoding: quoted-printable\n\nCaf=E9\n"
        b"--q\nContent-Type: text/html\n"
        b"Content-Transfer-Encoding: binary\n\n<p>\xff</p>\n"
        b"--q--\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    assert (tmp_path / "message.mime-pruned.eml").read_bytes() == raw


def test_malformed_content_id_fails_before_output(tmp_path: Path) -> None:
    raw = b"Content-Type: text/plain\r\nContent-ID: one-sided<\r\n\r\nbody\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()

"""Regression matrix for body-role preservation in the v3 MIME policy."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import BatchLedger, ItemStatus

if TYPE_CHECKING:
    from pathlib import Path


def _options(*, existing: str = "error") -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing=existing,
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


def test_keeps_conflicting_plain_and_html_representations_byte_exact(
    tmp_path: Path,
) -> None:
    raw = (
        b"MIME-Version: 1.0\r\n"
        b"Content-Type: multipart/alternative; boundary=x\r\n\r\n"
        b"--x\r\nContent-Type: text/plain\r\n\r\nThe goods are now here.\r\n"
        b"--x\r\nContent-Type: text/html\r\n\r\n<p>The goods are nowhere.</p>\r\n"
        b"--x--\r\n"
    )
    source, ledger = _run(tmp_path, raw)
    destination = tmp_path / "message.mime-pruned.eml"
    assert ledger.items[0].status is ItemStatus.CREATED
    assert destination.read_bytes() == raw
    assert hashlib.sha256(source.read_bytes()).digest() == hashlib.sha256(raw).digest()


def test_removes_only_explicit_attachment_and_preserves_html_bytes(
    tmp_path: Path,
) -> None:
    html = b'<p>body <img src="cid:gone"></p>\r\n'
    raw = (
        b"DKIM-Signature: pretend\r\n"
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/html\r\n\r\n"
        + html
        + b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment; filename=private.bin\r\n"
        b"Content-Transfer-Encoding: base64\r\n\r\nnot-valid-base64!\r\n"
        b"--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert ledger.items[0].status is ItemStatus.CREATED
    assert html in output
    assert b"private.bin" not in output
    assert b"DKIM-Signature" not in output


def test_related_prunes_nonroot_without_decoding_resource(tmp_path: Path) -> None:
    raw = (
        b'Content-Type: multipart/related; boundary=r; start="<root@example>"\r\n\r\n'
        b"--r\r\nContent-Type: text/html\r\nContent-ID: <root@example>\r\n\r\n"
        b"<p>retained</p>\r\n"
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <image@example>\r\n"
        b"Content-Transfer-Encoding: base64\r\n\r\ninvalid!\r\n"
        b"--r--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert ledger.items[0].status is ItemStatus.CREATED
    assert b"<p>retained</p>" in output
    assert b"image/png" not in output


def test_duplicate_mime_control_fails_before_output(tmp_path: Path) -> None:
    raw = b"Content-Type: text/plain\r\nContent-Type: text/html\r\n\r\nbody\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()


def test_verify_existing_requires_exact_current_candidate(tmp_path: Path) -> None:
    raw = b"Content-Type: text/plain\r\n\r\nbody\r\n"
    source, first = _run(tmp_path, raw)
    assert first.items[0].status is ItemStatus.CREATED
    second = execute([str(source)], _options(existing="verify"))
    assert second.items[0].status is ItemStatus.EXISTING_VERIFIED
    (tmp_path / "message.mime-pruned.eml").write_bytes(raw + b"changed")
    third = execute([str(source)], _options(existing="verify"))
    assert third.items[0].status is ItemStatus.FAILED


def test_unknown_charset_stays_opaque_and_batch_accounting_continues(
    tmp_path: Path,
) -> None:
    good = b"Content-Type: text/plain\r\n\r\ngood\r\n"
    opaque_charset = b"Content-Type: text/plain; charset=does-not-exist\r\n\r\ngood\r\n"
    sources = []
    for index, raw in enumerate((good, opaque_charset, good)):
        source = tmp_path / f"{index}.eml"
        source.write_bytes(raw)
        sources.append(str(source))
    ledger = execute(sources, _options())
    statuses = [item.status for item in ledger.items]
    assert statuses == [ItemStatus.CREATED] * 3, [item.error for item in ledger.items]


def test_retained_bad_cte_fails_only_its_batch_item(tmp_path: Path) -> None:
    good = b"Content-Type: text/plain\r\n\r\ngood\r\n"
    bad = b"Content-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n!\r\n"
    sources = []
    for index, raw in enumerate((good, bad, good)):
        source = tmp_path / f"{index}.eml"
        source.write_bytes(raw)
        sources.append(str(source))
    ledger = execute(sources, _options())
    statuses = [item.status for item in ledger.items]
    assert statuses == [
        ItemStatus.CREATED,
        ItemStatus.FAILED,
        ItemStatus.CREATED,
    ], [item.error for item in ledger.items]


def test_mixed_retains_ordered_inline_text_sections(tmp_path: Path) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nfirst\r\n"
        b"--m\r\nContent-Type: text/plain\r\n"
        b"Content-Disposition: inline\r\n\r\nsecond\r\n"
        b"--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert ledger.items[0].status is ItemStatus.CREATED
    assert output == raw
    assert output.index(b"first") < output.index(b"second")


def test_filename_only_text_role_is_rejected(tmp_path: Path) -> None:
    raw = b"Content-Type: text/plain; name=ambiguous.txt\r\n\r\nbody\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED


def test_unknown_inline_nontext_mixed_child_is_rejected(tmp_path: Path) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: inline\r\n\r\nbytes\r\n"
        b"--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED


def test_all_attachment_mixed_fails_without_empty_success(tmp_path: Path) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED

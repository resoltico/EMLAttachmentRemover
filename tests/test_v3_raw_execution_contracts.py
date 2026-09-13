"""Synthetic contracts for raw-byte execution, verification, and transfer encoding."""

from __future__ import annotations

import hashlib
from typing import SupportsIndex, overload

import pytest

from eml_attachment_remover import mime_encoding, mime_execution, mime_verification
from eml_attachment_remover.domain import AppError, ExitCode, Removal, RemovalReason
from eml_attachment_remover.mime_execution import Candidate, build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex


class _NoSliceBytes(bytes):
    """A byte sequence that makes accidental per-token tail copies observable."""

    @overload
    def __getitem__(self, index: SupportsIndex, /) -> int: ...

    @overload
    def __getitem__(self, index: slice, /) -> bytes: ...

    def __getitem__(self, index: SupportsIndex | slice, /) -> int | bytes:
        if isinstance(index, slice):
            message = "quoted-printable validation must not copy a tail slice"
            raise TypeError(message)
        return super().__getitem__(index)


def _mixed() -> bytes:
    return (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )


@pytest.mark.parametrize(
    ("encoded", "cte", "expected"),
    [
        (b"plain", "", b"plain"),
        (b"\xff", "8bit", b"\xff"),
        (b"\xff", "binary", b"\xff"),
        (b"a=\r\nb=\nc=3D", "quoted-printable", b"abc="),
    ],
)
def test_transfer_decoder_preserves_permitted_forms(
    encoded: bytes, cte: str, expected: bytes
) -> None:
    assert mime_encoding.decode_payload(encoded, cte) == expected


@pytest.mark.parametrize(
    ("encoded", "cte"),
    [(b"\xff", "7bit"), (b"!", "unknown"), (b"bad=", "quoted-printable")],
)
def test_transfer_decoder_rejects_invalid_forms(encoded: bytes, cte: str) -> None:
    with pytest.raises(AppError):
        mime_encoding.decode_payload(encoded, cte)


def test_quoted_printable_validation_uses_bounded_indexed_lookahead() -> None:
    """A repeated legal escape sequence must not allocate one tail per equals sign."""
    mime_encoding._validate_quoted_printable(  # ruff: ignore[private-member-access] - bounded transfer-encoding scan.
        _NoSliceBytes(b"=41" * 4096)
    )


def test_execution_rejects_invalid_plan_spans_and_removal_roots() -> None:
    with pytest.raises(AppError):
        mime_execution._nonoverlapping([(2, 2)])  # ruff: ignore[private-member-access] - direct raw-span invariant.
    with pytest.raises(AppError):
        mime_execution._nonoverlapping([(2, 5), (4, 6)])  # ruff: ignore[private-member-access] - direct raw-span invariant.
    tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    root = Removal((), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT)
    absent = Removal((99,), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT)
    for removal in (root, absent):
        with pytest.raises(AppError):
            build_candidate(tree, (removal,))
    with pytest.raises(AppError):
        RemovalIndex.from_roots({(1,), (1, 0)})


def test_verifier_rejects_digest_and_source_span_tampering() -> None:
    tree = parse_raw_mime(_mixed())
    policy = classify(tree.root)
    candidate = build_candidate(tree, policy.removals)
    roots = {removal.path for removal in policy.removals}
    digest_bad = Candidate(candidate.raw, "0" * 64, candidate.stripped_headers)
    with pytest.raises(AppError) as raised:
        mime_verification.verify_candidate(tree, digest_bad, roots)
    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    tampered = Candidate(
        candidate.raw + b"tamper",
        hashlib.sha256(candidate.raw + b"tamper").hexdigest(),
        candidate.stripped_headers,
    )
    with pytest.raises(AppError):
        mime_verification.verify_candidate(tree, tampered, roots)


def test_encoding_budget_and_nested_changed_header_contracts(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    with monkeypatch.context() as context:
        context.setattr(mime_encoding, "MAX_RETAINED_DECODED", 0)
        with pytest.raises(AppError):
            mime_encoding.fingerprint_retained(tree.raw, [tree.root])
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nouter\r\n"
        b"--m\r\nContent-Type: multipart/mixed; boundary=n\r\n"
        b"Content-MD5: stale\r\n\r\n"
        b"--n\r\nContent-Type: text/plain\r\n\r\ninner\r\n"
        b"--n\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--n--\r\n--m--\r\n"
    )
    nested = parse_raw_mime(raw)
    policy = classify(nested.root)
    candidate = build_candidate(nested, policy.removals)
    assert b"Content-MD5" not in candidate.raw
    receipt, _ = mime_verification.verify_candidate(
        nested, candidate, {removal.path for removal in policy.removals}
    )
    assert receipt.structure_matches


def test_verifier_rejects_root_and_overlapping_expected_spans() -> None:
    root_tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    with pytest.raises(AppError):
        mime_verification._expected_raw(root_tree, {()})  # ruff: ignore[private-member-access] - direct verifier span invariant.
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\nContent-MD5: stale\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    removed = tree.by_path[1,]
    stale = next(
        header for header in tree.root.headers if header.name == b"content-md5"
    )
    removed.delete_start = stale.start
    removed.delete_end = stale.end
    with pytest.raises(AppError):
        mime_verification._expected_raw(tree, {(1,)})  # ruff: ignore[private-member-access] - overlap verifier invariant.

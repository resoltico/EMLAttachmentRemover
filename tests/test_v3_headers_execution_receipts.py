"""Exact receipts for physical headers, raw candidate edits, and verifier parity."""

from __future__ import annotations

import ctypes
import hashlib
import os
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from eml_attachment_remover import (
    atomic_publish,
    mime_execution,
    mime_headers,
    mime_verification,
)
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_verification import verify_candidate


def _attachment_message() -> tuple[bytes, bytes]:
    """Return a source and its one permitted byte-deletion candidate.

    Returns:
        Source message bytes and the exact permitted candidate.

    """
    source = (
        b"DKIM-Signature: opaque\r\n"
        b"Content-Length: 999\r\n"
        b"Lines: 12\r\n"
        b"X-MS-Has-Attach: yes\r\n"
        b"Content-MD5: deadbeef\r\n"
        b"X-Keep: root\r\n"
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )
    expected = (
        b"X-Keep: root\r\n"
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n--m--\r\n"
    )
    return source, expected


def test_header_parser_preserves_mbox_relative_spans_and_physical_value_bytes() -> None:
    """The optional envelope is not a header and values lose only horizontal padding."""
    prefix = b"prefix"
    raw = prefix + b"From sender@example.test\r\nX-Token:\t value \v \t\r\n"
    start = len(prefix)
    headers = mime_headers.parse_headers(raw, start, len(raw))
    field_start = raw.index(b"X-Token:")
    assert headers == (
        mime_headers.Header(b"x-token", b"value \v", field_start, len(raw)),
    )


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        (b"X-Token:\t bounded \v \t", b"bounded \v"),
        (b"UPPER-9: value", b"value"),
    ],
)
def test_new_header_normalizes_only_name_and_horizontal_value_padding(
    line: bytes, expected: bytes
) -> None:
    """A physical header keeps all non-SP/TAB value bytes and its exact span."""
    assert mime_headers._new_header(  # ruff: ignore[private-member-access] - direct physical-field receipt.
        line, 17, 17 + len(line)
    ) == mime_headers.Header(
        line.partition(b":")[0].lower(), expected, 17, 17 + len(line)
    )
    for malformed in (b"NoColon", b"not a token: value"):
        with pytest.raises(AppError) as raised:
            mime_headers._new_header(  # ruff: ignore[private-member-access] - direct malformed-field receipt.
                malformed, 0, len(malformed)
            )
        assert raised.value == AppError(
            ExitCode.PARSE_ERROR, "malformed MIME header field"
        )


def test_execution_and_verifier_delete_the_same_authorized_wire_spans() -> None:
    """Candidate provenance, digest, and independent expected bytes are complete."""
    source, expected = _attachment_message()
    tree = parse_raw_mime(source)
    plan = classify(tree.root)
    roots = {removal.path for removal in plan.removals}
    candidate = build_candidate(tree, plan.removals)
    assert candidate.raw == expected
    assert candidate.digest == hashlib.sha256(expected).hexdigest()
    assert candidate.stripped_headers == (
        "content-length",
        "content-md5",
        "dkim-signature",
        "lines",
        "x-ms-has-attach",
    )
    assert (
        mime_verification._expected_raw(  # ruff: ignore[private-member-access] - independent verifier deletion receipt.
            tree, roots
        )
        == expected
    )
    receipt, retained = verify_candidate(tree, candidate, roots)
    assert (
        receipt.output_parses,
        receipt.retained_payloads_match,
        receipt.structure_matches,
        receipt.policy_is_idempotent,
        receipt.digest_matches,
    ) == (True, True, True, True, True)
    assert [(entry.source_path, entry.content_type) for entry in retained] == [
        ((0,), "text/plain")
    ]


def test_execution_sorting_accepts_adjacent_spans_without_coalescing() -> None:
    """Span normalization preserves every adjacent deletion as an exact edit."""
    assert mime_execution._nonoverlapping(  # ruff: ignore[private-member-access] - direct edit-plan receipt.
        [(8, 11), (2, 5), (5, 8)]
    ) == [(2, 5), (5, 8), (8, 11)]
    with pytest.raises(AppError) as raised:
        mime_execution._nonoverlapping(  # ruff: ignore[private-member-access] - negative raw edit rejection.
            [(-1, 2)]
        )
    assert raised.value == AppError(
        ExitCode.VERIFICATION_ERROR, "overlapping raw MIME edits"
    )


def test_darwin_publication_binds_exact_ctypes_abi_and_write_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The native call is byte-addressed and errors retain a user-safe native reason."""
    operation = Mock(return_value=-1)
    monkeypatch.setattr(
        ctypes,
        "CDLL",
        lambda *_args, **_kwargs: SimpleNamespace(renameatx_np=operation),
    )
    monkeypatch.setattr(ctypes, "get_errno", lambda: 5)
    monkeypatch.setattr(os, "strerror", lambda error: f"native-{error}")
    with pytest.raises(AppError) as raised:
        atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - direct Darwin ABI receipt.
            41, b"stage.eml", b"final.eml"
        )
    assert raised.value == AppError(
        ExitCode.WRITE_ERROR, "could not publish candidate: native-5"
    )
    assert operation.call_args.args == (41, b"stage.eml", 41, b"final.eml", 4)
    assert operation.argtypes == [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    assert operation.restype is ctypes.c_int


def test_portable_publication_uses_both_descriptor_relative_endpoints(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Portable publication never resolves either child name through process CWD."""
    link = Mock(side_effect=OSError(5, "permission denied"))
    monkeypatch.setattr(os, "link", link)
    with pytest.raises(AppError) as raised:
        atomic_publish._link_exclusive(  # ruff: ignore[private-member-access] - direct descriptor-relative publication receipt.
            29, b"candidate.eml", b"output.eml"
        )
    assert raised.value.code is ExitCode.WRITE_ERROR
    assert (
        str(raised.value) == "could not publish candidate: [Errno 5] permission denied"
    )
    assert link.call_args.args == (b"candidate.eml", b"output.eml")
    assert link.call_args.kwargs == {"src_dir_fd": 29, "dst_dir_fd": 29}

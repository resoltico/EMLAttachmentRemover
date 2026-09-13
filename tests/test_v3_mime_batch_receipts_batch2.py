"""Independent receipts for MIME physical ownership and proof work."""

from __future__ import annotations

import hashlib
from email.message import EmailMessage
from typing import cast

import pytest

from eml_attachment_remover import (
    mime_execution,
    mime_headers,
    mime_stdlib_check,
    mime_verification,
)
from eml_attachment_remover.domain import AppError, ExitCode, VerificationReceipt
from eml_attachment_remover.mime_execution import Candidate, build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex
from eml_attachment_remover.mime_stdlib_check import StdlibValidationWork


def test_line_end_observes_every_supported_newline_and_a_final_partial_line() -> None:
    """Physical line indexing must include exactly one available line ending."""
    raw = b"one\r\ntwo\nthree\rfour"
    assert [mime_headers.line_end(raw, start, len(raw)) for start in (0, 5, 9, 15)] == [
        5,
        9,
        15,
        len(raw),
    ]


def test_header_parser_keeps_envelope_outside_physical_ownership() -> None:
    """Only fields after an mbox envelope contribute indexed MIME ownership."""
    envelope = b"From sender@example.test Tue Jan 01 00:00:00 2030\r\n"
    raw = envelope + b"X-First: one \t\r\n\tsecond \t\r\n" + b"X-Second:\t two \t\r\n"
    first_start = len(envelope)
    second_start = raw.index(b"X-Second:")
    assert (
        mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - exact mbox-envelope receipt.
            raw, 0, len(raw)
        )
        == first_start
    )
    assert mime_headers.parse_headers(raw, 0, len(raw)) == (
        mime_headers.Header(
            b"x-first", b"one\r\n\tsecond \t", first_start, second_start
        ),
        mime_headers.Header(b"x-second", b"two", second_start, len(raw)),
    )
    assert (
        mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - non-envelope source must start at its caller-provided offset.
            b"Fromx: ordinary\r\n", 0, 16
        )
        == 0
    )


def test_header_append_enforces_blank_continuation_and_count_boundaries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Blank lines are inert, continuations require owners, and count is inclusive."""
    headers: list[mime_headers.Header] = []
    mime_headers._append_header(  # ruff: ignore[private-member-access] - blank physical-field receipt.
        headers, b"", 0, 0
    )
    assert headers == []
    with pytest.raises(AppError) as orphan:
        mime_headers._append_header(  # ruff: ignore[private-member-access] - orphan continuation receipt.
            headers, b"\tno owner", 0, 10
        )
    assert orphan.value == AppError(
        ExitCode.PARSE_ERROR, "orphaned MIME header continuation"
    )
    with monkeypatch.context() as context:
        context.setattr(mime_headers, "MAX_HEADERS", 1)
        mime_headers._append_header(  # ruff: ignore[private-member-access] - inclusive first physical-field receipt.
            headers, b"X-One: first", 3, 15
        )
        with pytest.raises(AppError) as count:
            mime_headers._append_header(  # ruff: ignore[private-member-access] - over-limit physical-field receipt.
                headers, b"X-Two: second", 15, 28
            )
    assert count.value == AppError(
        ExitCode.PARSE_ERROR, "MIME entity exceeds header-count limit"
    )
    assert [header.name for header in headers] == [b"x-one", b"x-two"]


def test_header_singleton_validation_rejects_only_declared_duplicate_controls() -> None:
    """Duplicate extension fields are physical evidence; duplicate controls are not."""
    extension = mime_headers.Header(b"x-repeat", b"one", 0, 14)
    mime_headers._validate_header_multiplicity(  # ruff: ignore[private-member-access] - extension duplicates remain permitted.
        [extension, mime_headers.Header(b"x-repeat", b"two", 14, 28)]
    )
    singleton = mime_headers.Header(b"content-transfer-encoding", b"7bit", 0, 33)
    with pytest.raises(AppError) as duplicate:
        mime_headers._validate_header_multiplicity(  # ruff: ignore[private-member-access] - MIME control uniqueness receipt.
            [singleton, singleton]
        )
    assert duplicate.value == AppError(
        ExitCode.PARSE_ERROR,
        "duplicate singleton MIME header content-transfer-encoding",
    )


def test_stdlib_crosscheck_counts_complete_nonopaque_tree_work() -> None:
    """The independent parser must check each ordinary node and each child edge."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\n\r\nplain\r\n"
        b"--inner\r\nContent-Type: text/html\r\n\r\n<html/>\r\n--inner--\r\n"
        b"--outer\r\nContent-Type: text/plain\r\n\r\ndirect\r\n--outer--\r\n"
    )
    tree = parse_raw_mime(raw)
    expected = StdlibValidationWork(node_visits=5, child_edge_visits=4)
    assert tree.stdlib_work == expected
    assert (
        mime_stdlib_check.validate_stdlib_tree(
            mime_stdlib_check.parse_stdlib(raw), tree.by_path
        )
        == expected
    )


def test_stdlib_children_accept_only_message_lists_and_preserve_order() -> None:
    """A payload list is valid only when every direct item is an EmailMessage."""
    first = EmailMessage()
    second = EmailMessage()
    container = EmailMessage()
    container.set_payload([first, second])
    assert mime_stdlib_check._children(  # ruff: ignore[private-member-access] - direct CPython-child receipt.
        container
    ) == [first, second]
    invalid = cast("EmailMessage", _PayloadPart([first, object()]))
    with pytest.raises(AppError) as rejected:
        mime_stdlib_check._children(  # ruff: ignore[private-member-access] - non-message CPython-child rejection.
            invalid
        )
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "stdlib parser has non-message child"
    )


def test_executor_and_verifier_strip_each_changed_ancestor_header_exactly() -> None:
    """Nested attachment deletion removes stale headers only from changed containers."""
    raw = (
        b"DKIM-Signature: root-proof\r\nContent-Length: 999\r\n"
        b"Lines: 9\r\nX-MS-Has-Attach: yes\r\nX-Root-Keep: retained\r\n"
        b"Content-Type: multipart/mixed; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Length: 77\r\nLines: 3\r\nX-Nested-Keep: retained\r\n"
        b"Content-Type: multipart/mixed; boundary=inner\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--inner\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--inner--\r\n--outer--\r\n"
    )
    tree = parse_raw_mime(raw)
    plan = classify(tree.root)
    assert [removal.path for removal in plan.removals] == [(0, 1)]
    edits, names = mime_execution._header_edits(  # ruff: ignore[private-member-access] - exact stale-header span receipt.
        tree, RemovalIndex.from_roots({(0, 1)})
    )
    assert sorted(
        (raw[start:end], name)
        for start, end in edits
        for name in [raw[start:end].split(b":", 1)[0].lower().decode("ascii")]
    ) == [
        (b"Content-Length: 77\r\n", "content-length"),
        (b"Content-Length: 999\r\n", "content-length"),
        (b"DKIM-Signature: root-proof\r\n", "dkim-signature"),
        (b"Lines: 3\r\n", "lines"),
        (b"Lines: 9\r\n", "lines"),
        (b"X-MS-Has-Attach: yes\r\n", "x-ms-has-attach"),
    ]
    assert sorted(names) == [
        "content-length",
        "content-length",
        "dkim-signature",
        "lines",
        "lines",
        "x-ms-has-attach",
    ]
    candidate = build_candidate(tree, plan.removals)
    assert candidate.stripped_headers == (
        "content-length",
        "dkim-signature",
        "lines",
        "x-ms-has-attach",
    )
    for stale in (
        b"DKIM-Signature:",
        b"Content-Length:",
        b"Lines:",
        b"X-MS-Has-Attach:",
        b"application/octet-stream",
        b"remove\r\n",
    ):
        assert stale not in candidate.raw
    assert b"X-Root-Keep: retained\r\n" in candidate.raw
    assert b"X-Nested-Keep: retained\r\n" in candidate.raw
    receipt, retained = mime_verification.verify_candidate(tree, candidate, {(0, 1)})
    assert receipt.digest_matches
    assert receipt.structure_matches
    assert [entry.source_path for entry in retained] == [(0, 0)]


def test_apply_and_empty_plan_verification_preserve_all_source_bytes() -> None:
    """A verifier proof with no removals remains a byte-identical idempotent receipt."""
    assert (
        mime_execution._apply(  # ruff: ignore[private-member-access] - exact noncontiguous raw-deletion receipt.
            b"0123456789", [(1, 3), (6, 8)]
        )
        == b"034589"
    )
    raw = b"X-Keep: value\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
    tree = parse_raw_mime(raw)
    candidate = Candidate(raw, hashlib.sha256(raw).hexdigest(), ())
    receipt, retained = mime_verification.verify_candidate(tree, candidate, set())
    assert receipt == VerificationReceipt(
        output_parses=True,
        retained_payloads_match=True,
        structure_matches=True,
        policy_is_idempotent=True,
        digest_matches=True,
    )
    assert [(entry.source_path, entry.content_type) for entry in retained] == [
        ((), "text/plain")
    ]


class _PayloadPart:
    """Tiny CPython-message double with an intentionally untyped payload list."""

    def __init__(self, payload: list[object]) -> None:
        self._payload = payload

    def get_payload(self) -> list[object]:
        """Return the caller-supplied fake parser payload.

        Returns:
            The exact fake parser payload.

        """
        return self._payload

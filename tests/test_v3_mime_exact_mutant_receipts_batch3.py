"""Exact public failure receipts for remaining MIME proof boundaries."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import (
    mime_execution,
    mime_headers,
    mime_stdlib_check,
    mime_verification,
)
from eml_attachment_remover.domain import AppError, ExitCode, Removal, RemovalReason
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import RawMimeTree, parse_raw_mime


def _attachment_tree() -> RawMimeTree:
    """Return one tree with exactly one removable direct attachment.

    Returns:
        The fully cross-checked source tree.

    """
    return parse_raw_mime(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )


def test_header_byte_limit_is_relative_inclusive_and_has_an_exact_receipt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A caller offset cannot consume header budget, while one excess byte fails."""
    with monkeypatch.context() as context:
        context.setattr(mime_headers, "MAX_HEADER_BYTES", 4)
        assert mime_headers.parse_headers(b"skipX: X", 4, 8) == (
            mime_headers.Header(b"x", b"X", 4, 8),
        )
        with pytest.raises(AppError) as over_limit:
            mime_headers.parse_headers(b"skipX: XX", 4, 9)
    assert over_limit.value == AppError(
        ExitCode.PARSE_ERROR, "MIME entity exceeds header-byte limit"
    )


def test_header_parser_uses_mbox_end_and_keeps_trailing_x_value() -> None:
    """The physical parser needs the provided end and strips no ordinary X bytes."""
    prefix = b"before"
    raw = prefix + b"From sender@example.test\r\nX-Token: X\r\n"
    start = len(prefix)
    token_start = raw.index(b"X-Token:")
    assert mime_headers.parse_headers(raw, start, len(raw)) == (
        mime_headers.Header(b"x-token", b"X", token_start, len(raw)),
    )


def test_header_parser_accepts_an_explicit_empty_header_region() -> None:
    """The physical parser has a valid zero-byte region at an entity boundary."""
    assert mime_headers.parse_headers(b"", 0, 0) == ()


def test_stdlib_tree_failure_paths_keep_their_exact_public_error_facts() -> None:
    """Every cross-check mismatch must retain its stable code and diagnostic."""
    with pytest.raises(AppError) as unindexed:
        mime_stdlib_check.validate_stdlib_tree(EmailMessage(), {})
    assert unindexed.value == AppError(
        ExitCode.PARSE_ERROR, "stdlib MIME tree has an unindexed part"
    )

    children_mismatch = _attachment_tree()
    children_mismatch.root.children.clear()
    with pytest.raises(AppError) as count_mismatch:
        mime_stdlib_check.validate_stdlib_tree(
            mime_stdlib_check.parse_stdlib(children_mismatch.raw),
            children_mismatch.by_path,
        )
    assert count_mismatch.value == AppError(
        ExitCode.PARSE_ERROR, "raw MIME child count disagrees with stdlib"
    )

    extra_index = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    extra_index.by_path[1,] = extra_index.root
    with pytest.raises(AppError) as unverified:
        mime_stdlib_check.validate_stdlib_tree(
            mime_stdlib_check.parse_stdlib(extra_index.raw), extra_index.by_path
        )
    assert unverified.value == AppError(
        ExitCode.PARSE_ERROR, "raw MIME index has unverified nodes"
    )


def test_candidate_builder_rejects_missing_root_root_node_and_partial_spans() -> None:
    """Every frozen deletion root must be indexed and carry both raw boundaries."""
    plain = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    absent = Removal((9,), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT)
    with pytest.raises(AppError) as unindexed:
        mime_execution.build_candidate(plain, (absent,))
    assert unindexed.value == AppError(
        ExitCode.VERIFICATION_ERROR, "removal root is unindexed"
    )
    root = Removal((), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT)
    with pytest.raises(AppError) as root_error:
        mime_execution.build_candidate(plain, (root,))
    assert root_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "policy attempted to remove root MIME node"
    )

    partial = _attachment_tree()
    removal = classify(partial.root).removals[0]
    partial.by_path[removal.path].delete_end = None
    with pytest.raises(AppError) as partial_span:
        mime_execution.build_candidate(partial, (removal,))
    assert partial_span.value == AppError(
        ExitCode.VERIFICATION_ERROR, "policy attempted to remove root MIME node"
    )


def test_verifier_rejects_partial_and_empty_deletion_spans_exactly() -> None:
    """Independent candidate proof refuses an absent boundary or a zero-width edit."""
    partial = _attachment_tree()
    removal = classify(partial.root).removals[0]
    partial.by_path[removal.path].delete_start = None
    with pytest.raises(AppError) as missing_span:
        mime_verification._expected_raw(  # ruff: ignore[private-member-access] - independent raw-span precondition receipt.
            partial, {removal.path}
        )
    assert missing_span.value == AppError(
        ExitCode.VERIFICATION_ERROR, "removal root lacks raw span"
    )

    empty = _attachment_tree()
    empty_removal = classify(empty.root).removals[0]
    node = empty.by_path[empty_removal.path]
    node.delete_end = node.delete_start
    with pytest.raises(AppError) as empty_span:
        mime_verification._expected_raw(  # ruff: ignore[private-member-access] - zero-width verifier-span receipt.
            empty, {empty_removal.path}
        )
    assert empty_span.value == AppError(
        ExitCode.VERIFICATION_ERROR, "invalid verifier deletion spans"
    )


def test_independent_verifier_proves_the_exact_authorized_deletion_boundary() -> None:
    """A valid direct attachment deletion preserves both retained wire boundaries."""
    tree = _attachment_tree()
    removal = classify(tree.root).removals[0]
    expected = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n--m--\r\n"
    )
    assert (
        mime_verification._expected_raw(  # ruff: ignore[private-member-access] - separately computed deletion receipt.
            tree, {removal.path}
        )
        == expected
    )

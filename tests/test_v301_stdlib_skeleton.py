"""Independent bounded-skeleton tests for opaque MIME source components."""

from __future__ import annotations

import pytest

from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_stdlib_skeleton import build_skeleton


def _mixed() -> bytes:
    return (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nprivate payload\r\n--m--\r\n"
    )


def _double_mixed() -> bytes:
    return (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nfirst\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nsecond\r\n--m--\r\n"
    )


def _related() -> bytes:
    return (
        b"Content-Type: multipart/related; boundary=r\r\n\r\n"
        b"--r\r\nContent-Type: text/plain\r\nContent-ID: <root@x>\r\n\r\nroot\r\n"
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <resource@x>\r\n\r\nbytes\r\n"
        b"--r--\r\n"
    )


def test_skeleton_replaces_only_opaque_payload_bytes() -> None:
    """Outer headers and multipart delimiters remain exact structural evidence."""
    raw = _mixed()
    tree = parse_raw_mime(raw)
    skeleton = build_skeleton(raw, tree.root)
    assert b"private payload" not in skeleton
    assert b"Content-Disposition: attachment\r\n" in skeleton
    assert b"--m--\r\n" in skeleton


def test_skeleton_rejects_forged_opaque_ownership_and_payload_spans() -> None:
    """A shared raw parser cannot silently authorize its own faulty span claim."""
    raw = _mixed()
    tree = parse_raw_mime(raw)
    attachment = tree.root.children[1]
    attachment.opaque = False
    with pytest.raises(AppError) as ownership:
        build_skeleton(raw, tree.root)
    assert ownership.value == AppError(
        ExitCode.PARSE_ERROR, "opaque raw-index ownership mismatch"
    )

    tree = parse_raw_mime(raw)
    attachment = tree.root.children[1]
    attachment.body_start = -1
    with pytest.raises(AppError) as span:
        build_skeleton(raw, tree.root)
    assert span.value == AppError(ExitCode.PARSE_ERROR, "invalid opaque skeleton span")

    tree = parse_raw_mime(raw)
    tree.root.children[1].payload_end = None
    with pytest.raises(AppError) as unconfined:
        build_skeleton(raw, tree.root)
    assert unconfined.value == AppError(
        ExitCode.PARSE_ERROR, "opaque payload has no confined span"
    )


def test_skeleton_rejects_an_unselected_related_root_before_replacement() -> None:
    """Source related controls must select one direct retained root independently."""
    raw = _related()
    tree = parse_raw_mime(raw)
    tree.root.content_type.parameters[b"start"] = b"<missing@x>"
    with pytest.raises(AppError) as rejected:
        build_skeleton(raw, tree.root)
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "related start has no unique direct root"
    )


def test_skeleton_preserves_each_edit_boundary_and_separator_exactly() -> None:
    """Two independent opaque payloads become exactly one placeholder each."""
    raw = _double_mixed()
    skeleton = build_skeleton(raw, parse_raw_mime(raw).root)
    assert skeleton == raw.replace(b"first", b"X").replace(b"second", b"X")


def test_skeleton_validates_zero_and_terminal_opaque_spans_independently() -> None:
    """Skeleton span checks retain inclusive lower and upper byte boundaries."""
    raw = _mixed()
    tree = parse_raw_mime(raw)
    attachment = tree.root.children[1]
    attachment.body_start = 0
    attachment.payload_end = 0
    attachment.end = len(raw)
    assert build_skeleton(raw, tree.root) == b"X" + raw

    tree = parse_raw_mime(raw)
    attachment = tree.root.children[1]
    attachment.payload_end = len(raw)
    attachment.end = len(raw)
    assert build_skeleton(raw, tree.root).endswith(b"X")

    tree = parse_raw_mime(raw)
    attachment = tree.root.children[1]
    attachment.payload_end = attachment.body_start - 1
    with pytest.raises(AppError) as reversed_span:
        build_skeleton(raw, tree.root)
    assert reversed_span.value == AppError(
        ExitCode.PARSE_ERROR, "opaque payload has no confined span"
    )


def test_skeleton_preserves_related_identifier_error_contexts() -> None:
    """Independent related-root selection keeps both raw identifier labels exact."""
    raw = _related()
    tree = parse_raw_mime(raw)
    tree.root.content_type.parameters[b"start"] = b"<missing"
    with pytest.raises(AppError) as start:
        build_skeleton(raw, tree.root)
    assert start.value == AppError(
        ExitCode.PARSE_ERROR, "malformed multipart/related start"
    )

    tree = parse_raw_mime(raw)
    tree.root.content_type.parameters[b"start"] = b"<root@x>"
    tree.root.children[0].headers = (Header(b"content-id", b"<broken", 0, 18),)
    with pytest.raises(AppError) as content_identifier:
        build_skeleton(raw, tree.root)
    assert content_identifier.value == AppError(
        ExitCode.PARSE_ERROR, "malformed Content-ID"
    )


def test_skeleton_rejects_invalid_edit_order_before_replacing_bytes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Independent skeleton edits cannot overlap or run backward."""
    tree = parse_raw_mime(_mixed())
    monkeypatch.setattr(
        "eml_attachment_remover.mime_stdlib_skeleton._opaque_payload_edits",
        lambda _root: [(2, 1)],
    )
    with pytest.raises(AppError) as rejected:
        build_skeleton(b"abc", tree.root)
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "invalid opaque skeleton span"
    )

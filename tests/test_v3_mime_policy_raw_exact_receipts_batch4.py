"""Exact policy and raw-wire receipts for live mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_policy, mime_raw
from eml_attachment_remover.domain import AppError, DecisionAction, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def _node(
    path: tuple[int, ...],
    media_type: str,
    *,
    headers: tuple[Header, ...] = (),
    disposition: str | None = None,
) -> mime_raw.RawNode:
    """Build a direct policy/raw node with only the requested role facts.

    Returns:
        One mutable raw node suitable for an isolated policy receipt.

    """
    return mime_raw.RawNode(
        path,
        0,
        0,
        0,
        headers,
        ContentSpec(media_type, {}),
        None if disposition is None else ContentSpec(disposition, {}),
        "7bit",
    )


def test_policy_content_id_preserves_presence_and_exact_malformed_field_name() -> None:
    """Policy identity lookup returns exact identifiers and surfaces field context."""
    valid = _node(
        (),
        "text/plain",
        headers=(Header(b"content-id", b"(note) <body@x>\t", 0, 28),),
    )
    assert mime_policy._cid(valid) == b"body@x"  # ruff: ignore[private-member-access] - exact policy Content-ID receipt.
    assert mime_policy._cid(_node((), "text/plain")) is None  # ruff: ignore[private-member-access] - absent policy Content-ID receipt.
    malformed = _node(
        (),
        "text/plain",
        headers=(Header(b"content-id", b"not-bracketed", 0, 13),),
    )
    with pytest.raises(AppError) as rejected:
        mime_policy._cid(malformed)  # ruff: ignore[private-member-access] - malformed policy Content-ID field receipt.
    assert rejected.value == AppError(ExitCode.PARSE_ERROR, "malformed Content-ID")


def test_policy_related_and_final_leaf_failures_preserve_full_error_records() -> None:
    """Related emptiness and an all-pruned mixed body have distinct terminal facts."""
    related = _node((4,), "multipart/related")
    with pytest.raises(AppError) as empty_related:
        mime_policy.classify(related)
    assert empty_related.value == AppError(
        ExitCode.TRANSFORMATION_UNAVAILABLE,
        "empty multipart/related",
        (4,),
    )

    mixed = _node((), "multipart/mixed")
    attachment = _node((0,), "application/octet-stream", disposition="attachment")
    mixed.children = [attachment]
    with pytest.raises(AppError) as no_body:
        mime_policy.classify(mixed)
    assert no_body.value == AppError(
        ExitCode.TRANSFORMATION_UNAVAILABLE, "no supported body remains"
    )


def test_first_line_header_detection_respects_bounds_and_first_colon() -> None:
    """Only the bounded first line and its first colon define header-like syntax."""
    assert not mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - bounded initial-line receipt.
        b"plain: later\r\n", 0, 5
    )
    assert mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - first-colon physical-header receipt.
        b"X-Trace: first: second\r\n", 0, 24
    )


def test_separator_search_observes_range_and_a_separator_at_zero() -> None:
    """Separators outside a bounded entity are absent, while index zero is valid."""
    with pytest.raises(AppError) as bounded_absence:
        mime_raw._find_separator(  # ruff: ignore[private-member-access] - bounded separator-search receipt.
            b"abc\r\n\r\nbody", 0, 3
        )
    assert bounded_absence.value == AppError(
        ExitCode.PARSE_ERROR, "MIME entity has no header/body separator"
    )
    assert mime_raw._find_separator(  # ruff: ignore[private-member-access] - zero-offset separator receipt.
        b"\r\n\r\nbody", 0, 8
    ) == (0, 4)


def test_delimiter_scanner_rejects_nontransport_tails() -> None:
    """Only SP/TAB may follow a delimiter; closing consumes exactly two hyphens."""
    assert mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - ordinary opening/closing span receipt.
        b"--m\r\n--m--\r\n", 0, 12, b"m"
    ) == [(0, 5, False), (5, 12, True)]
    for raw in (b"--m--x\r\n", b"--m\v\r\n"):
        assert (
            mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - nontransport delimiter-tail rejection.
                raw, 0, len(raw), b"m"
            )
            == []
        )


def test_delimiter_scanner_accepts_an_explicit_empty_range() -> None:
    """An empty body range has no delimiter records and is not a cursor failure."""
    assert (
        mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - zero-range delimiter receipt.
            b"", 0, 0, b"m"
        )
        == []
    )


def test_raw_content_id_uses_the_content_id_error_context() -> None:
    """Raw-tree child identity parsing carries the same public Content-ID field name."""
    valid = _node(
        (0,),
        "text/plain",
        headers=(Header(b"content-id", b"<retained@x>", 0, 18),),
    )
    assert mime_raw._content_id(valid) == b"retained@x"  # ruff: ignore[private-member-access] - raw Content-ID positive receipt.
    malformed = _node(
        (0,),
        "text/plain",
        headers=(Header(b"content-id", b"missing brackets", 0, 16),),
    )
    with pytest.raises(AppError) as rejected:
        mime_raw._content_id(malformed)  # ruff: ignore[private-member-access] - raw Content-ID field receipt.
    assert rejected.value == AppError(ExitCode.PARSE_ERROR, "malformed Content-ID")


def test_classifier_records_explicit_removal_before_no_body_failure() -> None:
    """The no-body failure follows a recorded explicit attachment decision."""
    classifier = mime_policy._Classifier()  # ruff: ignore[private-member-access] - direct policy decision receipt.
    mixed = _node((), "multipart/mixed")
    attachment = _node((0,), "application/octet-stream", disposition="attachment")
    mixed.children = [attachment]
    with pytest.raises(AppError):
        classifier.classify(mixed)
    assert classifier.actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.REMOVE_SUBTREE,
    }

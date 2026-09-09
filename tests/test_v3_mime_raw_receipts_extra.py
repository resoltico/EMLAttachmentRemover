"""Exact low-level receipts for raw MIME-tree boundary decisions."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_raw import RawNode
from eml_attachment_remover.mime_validation import ContentSpec


def _node(
    path: tuple[int, ...],
    media_type: str,
    parameters: dict[bytes, bytes] | None = None,
    headers: tuple[Header, ...] = (),
) -> RawNode:
    """Make a direct raw-index node whose structural controls are explicit.

    Returns:
        One raw node with exactly the requested path and MIME controls.

    """
    return RawNode(
        path,
        0,
        0,
        0,
        headers,
        ContentSpec(media_type, {} if parameters is None else parameters),
        None,
        "7bit",
    )


def test_count_node_preserves_exact_limits_counters_and_failure_paths(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Node and depth budgets are inclusive and leave other totals untouched."""
    totals = [1, 41]
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_NODES", 2)
        mime_raw._count_node((0,), totals)  # ruff: ignore[private-member-access] - exact raw-index budget receipt.
        assert totals == [2, 41]
        with pytest.raises(AppError) as raised:
            mime_raw._count_node((1,), totals)  # ruff: ignore[private-member-access] - exact raw-index budget failure.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "MIME tree exceeds node limit", (1,)
    )
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_DEPTH", 1)
        mime_raw._count_node((0,), [0, 0])  # ruff: ignore[private-member-access] - inclusive raw-index depth receipt.
        with pytest.raises(AppError) as raised:
            mime_raw._count_node((0, 1), [0, 0])  # ruff: ignore[private-member-access] - exact raw-index depth failure.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "MIME tree exceeds depth limit", (0, 1)
    )


def test_separator_and_entity_header_receipts_preserve_wire_boundaries() -> None:
    """Each transport separator and headerless-body decision remains byte exact."""
    assert mime_raw._find_separator(b"X: y\r\n\r\nbody", 0, 13) == (4, 4)  # ruff: ignore[private-member-access] - exact entity separator receipt.
    assert mime_raw._find_separator(b"X: y\n\nbody", 0, 10) == (4, 2)  # ruff: ignore[private-member-access] - exact entity separator receipt.
    assert mime_raw._find_separator(b"X: y\r\rbody", 0, 10) == (4, 2)  # ruff: ignore[private-member-access] - exact entity separator receipt.
    raw = b"X-Role: first\r\n\tcontinued\r\n\r\nbody"
    assert mime_raw._entity_headers(raw, 0, len(raw)) == (  # ruff: ignore[private-member-access] - exact entity header receipt.
        (Header(b"x-role", b"first\r\n\tcontinued", 0, 25),),
        29,
        25,
    )
    headerless = b"plain prose\r\n\r\nX: later"
    assert mime_raw._entity_headers(headerless, 0, len(headerless)) == (  # ruff: ignore[private-member-access] - headerless entity receipt.
        (),
        0,
        0,
    )


def test_separator_and_header_like_entity_failures_are_publicly_exact() -> None:
    """Malformed separator states cannot silently become body text."""
    with pytest.raises(AppError) as raised:
        mime_raw._find_separator(b"X: y\r\nbody", 0, 10)  # ruff: ignore[private-member-access] - exact separator failure.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "MIME entity has no header/body separator"
    )
    with pytest.raises(AppError) as raised:
        mime_raw._entity_headers(b"X-Role: first\r\nbody", 0, 20)  # ruff: ignore[private-member-access] - header-like entity must fail closed.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "header-like MIME entity lacks a body separator"
    )


def test_opening_delimiter_acceptance_and_each_invalid_terminal_ordering() -> None:
    """Only one final closing marker may terminate a nonempty child sequence."""
    records = [(1, 5, False), (9, 13, False), (17, 23, True)]
    assert mime_raw._opening_delimiters(records, (4,)) == records[:-1]  # ruff: ignore[private-member-access] - exact multipart opening receipt.
    invalid = [
        ([], "multipart entity lacks a closing boundary"),
        ([(1, 5, False)], "multipart entity lacks a closing boundary"),
        ([(1, 7, True)], "multipart entity has no child parts"),
        (
            [(1, 5, False), (9, 15, True), (17, 21, False), (25, 31, True)],
            "multipart content follows a closing boundary",
        ),
    ]
    for delimiters, message in invalid:
        with pytest.raises(AppError) as raised:
            mime_raw._opening_delimiters(delimiters, (4,))  # ruff: ignore[private-member-access] - exact multipart boundary failure.
        assert raised.value == AppError(ExitCode.PARSE_ERROR, message, (4,))


def test_parse_multipart_builds_exact_direct_child_receipts() -> None:
    """Direct children retain their source, deletion, and payload boundaries."""
    raw = (
        b"--m\r\nContent-Type: text/plain\r\n\r\none\r\n"
        b"--m\r\nContent-Type: text/html\r\n\r\ntwo\r\n--m--\r\n"
    )
    parent = _node((), "multipart/mixed", {b"boundary": b"m"})
    parent.end = len(raw)
    mime_raw._parse_multipart(raw, parent, [1, 0])  # ruff: ignore[private-member-access] - direct multipart indexing receipt.
    first = raw.index(b"--m\r\n")
    second = raw.index(b"--m\r\n", first + 1)
    closing = raw.index(b"--m--\r\n")
    assert [
        (
            child.path,
            child.start,
            child.end,
            child.body_start,
            child.delete_start,
            child.delete_end,
            child.payload_end,
            child.media_type,
            child.opaque,
        )
        for child in parent.children
    ] == [
        (
            (0,),
            first + 5,
            second,
            first + 33,
            first,
            second,
            second - 2,
            "text/plain",
            False,
        ),
        (
            (1,),
            second + 5,
            closing,
            second + 32,
            second,
            closing,
            closing - 2,
            "text/html",
            False,
        ),
    ]


def test_parse_multipart_rejects_missing_boundaries_and_child_budget_exactly(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A multipart control cannot omit its boundary or exceed the child ceiling."""
    missing = _node((2,), "multipart/mixed")
    with pytest.raises(AppError) as raised:
        mime_raw._parse_multipart(b"", missing, [1, 0])  # ruff: ignore[private-member-access] - direct missing-boundary failure.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "multipart entity lacks a boundary", (2,)
    )
    raw = b"--m\r\n\r\none\r\n--m--\r\n"
    limited = _node((2,), "multipart/mixed", {b"boundary": b"m"})
    limited.end = len(raw)
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_CHILDREN", 0)
        with pytest.raises(AppError) as raised:
            mime_raw._parse_multipart(raw, limited, [1, 0])  # ruff: ignore[private-member-access] - direct multipart child budget failure.
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "multipart exceeds direct-child limit", (2,)
    )


def test_related_root_selection_is_exact_for_context_default_and_identifiers() -> None:
    """Related roots select only the declared unique direct Content-ID child."""
    plain = _node((0,), "text/plain")
    root = _node(
        (3,),
        "multipart/related",
        {b"start": b"<render@x>"},
    )
    render = _node(
        (1,),
        "text/html",
        headers=(Header(b"content-id", b"<render@x>", 0, 21),),
    )
    assert mime_raw._related_root_index(_node((), "multipart/mixed"), [plain]) is None  # ruff: ignore[private-member-access] - nonrelated root context receipt.
    assert mime_raw._related_root_index(_node((), "multipart/related"), []) == 0  # ruff: ignore[private-member-access] - related default-root receipt.
    assert mime_raw._related_root_index(root, [plain, render]) == 1  # ruff: ignore[private-member-access] - related identifier-root receipt.


def test_related_root_selection_rejects_missing_and_duplicate_identifiers() -> None:
    """A related ``start`` reference must select exactly one direct child."""
    parent = _node((7,), "multipart/related", {b"start": b"<render@x>"})
    matching = _node(
        (0,),
        "text/plain",
        headers=(Header(b"content-id", b"<render@x>", 0, 21),),
    )
    for children in ([], [matching, matching]):
        with pytest.raises(AppError) as raised:
            mime_raw._related_root_index(parent, children)  # ruff: ignore[private-member-access] - unique related-root failure.
        assert raised.value == AppError(
            ExitCode.PARSE_ERROR, "related start has no unique direct root", (7,)
        )

"""Complete structural receipts for raw MIME tree indexing."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_multipart_tree_has_exact_source_and_deletion_span_receipt() -> None:
    """Index every retained multipart node at its exact source and deletion spans."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=outer\r\n\r\n"
        b"outer preamble\r\n"
        b"--outer\r\nContent-Type: text/plain\r\nX-Role: first\r\n\r\nfirst\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n"
        b"inner preamble\r\n"
        b"--inner\r\nContent-Type: text/plain\r\n\r\nplain\r\n"
        b"--inner\r\nContent-Type: text/html\r\n\r\n<html/>\r\n"
        b"--inner--\r\ninner epilogue\r\n"
        b"--outer--\r\nouter epilogue\r\n"
    )
    tree = parse_raw_mime(raw)
    outer = [
        raw.index(b"--outer\r\n"),
        raw.index(b"--outer\r\n", raw.index(b"--outer\r\n") + 1),
        raw.index(b"--outer--\r\n"),
    ]
    inner = [
        raw.index(b"--inner\r\n"),
        raw.index(b"--inner\r\n", raw.index(b"--inner\r\n") + 1),
        raw.index(b"--inner--\r\n"),
    ]
    first_start = outer[0] + len(b"--outer\r\n")
    nested_start = outer[1] + len(b"--outer\r\n")
    plain_start = inner[0] + len(b"--inner\r\n")
    html_start = inner[1] + len(b"--inner\r\n")
    expected = [
        ((), 0, len(raw), raw.index(b"\r\n\r\n") + 4, None, None, None, False),
        (
            (0,),
            first_start,
            outer[1],
            raw.index(b"\r\n\r\n", first_start) + 4,
            outer[0],
            outer[1],
            outer[1] - 2,
            False,
        ),
        (
            (1,),
            nested_start,
            outer[2],
            raw.index(b"\r\n\r\n", nested_start) + 4,
            outer[1],
            outer[2],
            outer[2] - 2,
            False,
        ),
        (
            (1, 0),
            plain_start,
            inner[1],
            raw.index(b"\r\n\r\n", plain_start) + 4,
            inner[0],
            inner[1],
            inner[1] - 2,
            False,
        ),
        (
            (1, 1),
            html_start,
            inner[2],
            raw.index(b"\r\n\r\n", html_start) + 4,
            inner[1],
            inner[2],
            inner[2] - 2,
            False,
        ),
    ]
    assert [
        (
            node.path,
            node.start,
            node.end,
            node.body_start,
            node.delete_start,
            node.delete_end,
            node.payload_end,
            node.opaque,
        )
        for node in tree.nodes
    ] == expected
    assert [node.path for node in tree.root.children] == [(0,), (1,)]
    assert [node.path for node in tree.by_path.values()] == [
        (),
        (0,),
        (1,),
        (1, 0),
        (1, 1),
    ]
    assert tree.by_path[0,].headers[1].value == b"first"


def test_opening_delimiter_receipts_preserve_openers_and_reject_late_content() -> None:
    """Separate usable opening records from a terminal closing delimiter exactly."""
    delimiters = [(11, 20, False), (42, 51, False), (74, 85, True)]
    assert mime_raw._opening_delimiters(delimiters, (3,)) == delimiters[:-1]  # ruff: ignore[private-member-access] - direct parser delimiter receipt.
    with pytest.raises(AppError) as raised:
        mime_raw._opening_delimiters(  # ruff: ignore[private-member-access] - direct parser delimiter receipt.
            [(11, 20, False), (42, 53, True), (53, 62, False), (62, 73, True)],
            (3,),
        )
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "multipart content follows a closing boundary", (3,)
    )


def test_header_budget_accumulates_every_parsed_entity_exactly(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Accept the exact cumulative header limit and reject one byte beyond it."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\nX-Role: one\r\n\r\none\r\n"
        b"--m\r\nContent-Type: text/html\r\nX-Role: two\r\n\r\ntwo\r\n"
        b"--m--\r\n"
    )
    first_start = raw.index(b"--m\r\n") + len(b"--m\r\n")
    second_start = raw.index(b"--m\r\n", first_start) + len(b"--m\r\n")
    total_headers = (
        raw.index(b"\r\n\r\n")
        + raw.index(b"\r\n\r\n", first_start)
        - first_start
        + raw.index(b"\r\n\r\n", second_start)
        - second_start
    )
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_TOTAL_HEADERS", total_headers)
        assert len(parse_raw_mime(raw).nodes) == 3
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_TOTAL_HEADERS", total_headers - 1)
        with pytest.raises(AppError) as raised:
            parse_raw_mime(raw)
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "MIME message exceeds cumulative header limit"
    )


def test_related_start_selects_one_direct_root_and_leaves_nonroots_opaque() -> None:
    """Use the declared direct Content-ID root before descending into its children."""
    raw = (
        b'Content-Type: multipart/related; boundary=related; start="<render@x>"\r\n\r\n'
        b"--related\r\nContent-Type: multipart/mixed; boundary=unclosed\r\n"
        b"Content-ID: <resource@x>\r\n\r\nnot a complete nested multipart\r\n"
        b"--related\r\nContent-Type: multipart/alternative; boundary=alt\r\n"
        b"Content-ID: <render@x>\r\n\r\n"
        b"--alt\r\nContent-Type: text/plain\r\n\r\nplain\r\n"
        b"--alt\r\nContent-Type: text/html\r\n\r\n<html/>\r\n--alt--\r\n"
        b"--related\r\nContent-Type: image/png\r\n"
        b"Content-ID: <trailing@x>\r\n\r\nimage\r\n--related--\r\n"
    )
    tree = parse_raw_mime(raw)
    assert [node.path for node in tree.nodes] == [
        (),
        (0,),
        (1,),
        (1, 0),
        (1, 1),
        (2,),
    ]
    assert [(node.path, node.opaque) for node in tree.root.children] == [
        ((0,), True),
        ((1,), False),
        ((2,), True),
    ]
    assert tree.by_path[0,].children == []
    assert [node.content_type.token for node in tree.by_path[1,].children] == [
        "text/plain",
        "text/html",
    ]

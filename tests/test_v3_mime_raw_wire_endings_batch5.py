"""Physical first-line and delimiter-index receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw
from eml_attachment_remover.mime_raw import parse_raw_mime


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        (b"X-Token: value\r\n", b"X-Token: value"),
        (b"X-Token: value\r", b"X-Token: value"),
        (b"X-Token: value\n", b"X-Token: value"),
        (b"X-Token: value", b"X-Token: value"),
    ],
)
def test_physical_line_endings_leave_the_exact_header_name(
    line: bytes, expected: bytes
) -> None:
    """Only the final transport ending is excluded before a field name is parsed."""
    assert expected == b"X-Token: value"
    assert mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - physical header-line receipt.
        line, 0, len(line)
    )


def test_shallow_child_boundaries_follow_the_next_delimiter_index() -> None:
    """Each child ends at the next recognized delimiter, without parallel zip state."""
    tree = parse_raw_mime(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\none\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\ntwo\r\n--m--\r\n"
    )
    first_delimiter = tree.raw.index(b"--m\r\n")
    second_delimiter = tree.raw.index(b"--m\r\n", first_delimiter + 1)
    closing_delimiter = tree.raw.index(b"--m--\r\n")
    assert [
        (child.path, child.body_start, child.end) for child in tree.root.children
    ] == [
        ((0,), tree.raw.index(b"\r\none") + 2, second_delimiter),
        (
            (1,),
            tree.raw.index(b"\r\ntwo") + 2,
            closing_delimiter,
        ),
    ]

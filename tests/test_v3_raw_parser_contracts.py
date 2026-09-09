"""Synthetic raw-index boundary, limit, and delimiter contracts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_raw_helpers_cover_non_delimiter_payload_and_opening_errors() -> None:
    assert mime_raw._payload_end(b"abc--", 3) == 3  # ruff: ignore[private-member-access] - raw delimiter boundary contract.
    assert mime_raw._delimiter_lines(b"--mX\r\n--m--\r\n", 0, 12, b"m") == [  # ruff: ignore[private-member-access] - exact delimiter-token contract.
        (6, 13, True)
    ]
    for delimiters in ([], [(0, 3, True)], [(0, 3, True), (3, 6, True)]):
        with pytest.raises(AppError):
            mime_raw._opening_delimiters(delimiters, ())  # ruff: ignore[private-member-access] - direct delimiter-state contract.


def test_delimiter_scan_rejects_a_nonadvancing_wire_cursor(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Fail closed rather than spin when a mutated delimiter cursor regresses."""
    monkeypatch.setattr(mime_raw, "line_end", lambda _raw, position, _end: position)
    with pytest.raises(AppError) as captured:
        mime_raw._delimiter_lines(b"--m\r\n", 0, 5, b"m")  # ruff: ignore[private-member-access] - delimiter progress invariant.
    assert captured.value == AppError(
        ExitCode.PARSE_ERROR, "MIME delimiter cursor did not advance"
    )
    monkeypatch.undo()
    assert mime_raw._advanced_delimiter_cursor(3, 4) == 4  # ruff: ignore[private-member-access] - direct delimiter progress receipt.
    with pytest.raises(AppError) as captured:
        mime_raw._advanced_delimiter_cursor(4, 4)  # ruff: ignore[private-member-access] - equal delimiter cursor must fail closed.
    assert captured.value == AppError(
        ExitCode.PARSE_ERROR, "MIME delimiter cursor did not advance"
    )
    monkeypatch.setattr(
        mime_raw, "_advanced_delimiter_cursor", lambda position, _next: position
    )
    with pytest.raises(AppError) as captured:
        mime_raw._delimiter_lines(b"x", 0, 1, b"m")  # ruff: ignore[private-member-access] - bounded delimiter progress invariant.
    assert captured.value == AppError(
        ExitCode.PARSE_ERROR, "MIME delimiter cursor did not advance"
    )


@pytest.mark.parametrize(
    ("raw", "boundary_start", "expected"),
    [(b"x\r\n--", 3, 1), (b"x\n--", 2, 1), (b"x\r--", 2, 1), (b"x--", 1, 1)],
)
def test_payload_end_removes_only_the_immediate_transport_line_ending(
    raw: bytes, boundary_start: int, expected: int
) -> None:
    """Require exact CRLF/LF/CR payload trimming at a delimiter boundary."""
    assert mime_raw._payload_end(raw, boundary_start) == expected  # ruff: ignore[private-member-access] - exact payload boundary.


def test_raw_parser_rejects_limits_missing_boundary_and_related_start(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_TOTAL_HEADERS", 0)
        with pytest.raises(AppError):
            parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_DEPTH", -1)
        with pytest.raises(AppError):
            mime_raw._count_node((), [0, 0])  # ruff: ignore[private-member-access] - depth budget contract.
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_NODES", 0)
        with pytest.raises(AppError):
            mime_raw._count_node((), [0, 0])  # ruff: ignore[private-member-access] - node budget contract.
    for raw in (
        b"Content-Type: multipart/mixed\r\n\r\nbody\r\n",
        (
            b'Content-Type: multipart/related; boundary=r; start="<missing@x>"\r\n\r\n'
            b"--r\r\nContent-Type: text/plain\r\n\r\nx\r\n--r--\r\n"
        ),
    ):
        with pytest.raises(AppError):
            parse_raw_mime(raw)


def test_raw_parser_rejects_direct_child_budget() -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nx\r\n--m--\r\n"
    )
    with pytest.MonkeyPatch.context() as context:
        context.setattr(mime_raw, "MAX_CHILDREN", 0)
        with pytest.raises(AppError):
            parse_raw_mime(raw)

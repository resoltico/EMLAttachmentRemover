"""Synthetic raw-index boundary, limit, and delimiter contracts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw
from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_raw_helpers_cover_non_delimiter_payload_and_opening_errors() -> None:
    assert mime_raw._payload_end(b"abc--", 3) == 3  # ruff: ignore[private-member-access] - raw delimiter boundary contract.
    assert mime_raw._delimiter_lines(b"--mX\r\n--m--\r\n", 0, 12, b"m") == [  # ruff: ignore[private-member-access] - exact delimiter-token contract.
        (6, 13, True)
    ]
    for delimiters in ([], [(0, 3, True)], [(0, 3, True), (3, 6, True)]):
        with pytest.raises(AppError):
            mime_raw._opening_delimiters(delimiters, ())  # ruff: ignore[private-member-access] - direct delimiter-state contract.


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

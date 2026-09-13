"""Synthetic independent-stdlib ownership and defect contracts."""

from __future__ import annotations

from email.message import EmailMessage
from types import SimpleNamespace
from typing import cast

import pytest

from eml_attachment_remover import mime_stdlib_check
from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_stdlib_parser_wraps_parser_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    class BrokenParser:
        def __init__(self, *, policy: object) -> None:
            del policy

        @staticmethod
        def parsebytes(_raw: bytes) -> EmailMessage:
            message = "broken"
            raise ValueError(message)

    monkeypatch.setattr(mime_stdlib_check, "BytesParser", BrokenParser)
    with pytest.raises(AppError):
        mime_stdlib_check.parse_stdlib(b"body")


def test_stdlib_tree_rejects_missing_nodes_children_and_unvisited_nodes() -> None:
    message = EmailMessage()
    with pytest.raises(AppError):
        mime_stdlib_check.validate_stdlib_tree(message, {})
    tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    extra = dict(tree.by_path)
    extra[1,] = tree.root
    with pytest.raises(AppError):
        mime_stdlib_check.validate_stdlib_tree(message, extra)
    multi = EmailMessage()
    multi.set_payload([EmailMessage()])
    with pytest.raises(AppError):
        mime_stdlib_check.validate_stdlib_tree(multi, {(): tree.root})


def test_stdlib_helpers_reject_nonmessage_children_and_header_ownership() -> None:
    nonmessage = SimpleNamespace(get_payload=lambda: [object()])
    with pytest.raises(AppError):
        mime_stdlib_check._children(cast("EmailMessage", nonmessage))  # ruff: ignore[private-member-access] - direct stdlib child invariant.
    tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    mismatched_headers = SimpleNamespace(
        raw_items=list, get_content_type=lambda: "text/plain"
    )
    with pytest.raises(AppError):
        mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - direct ownership invariant.
            cast("EmailMessage", mismatched_headers), tree.root
        )
    wrong_type = SimpleNamespace(
        raw_items=lambda: [("Content-Type", "text/plain")],
        get_content_type=lambda: "text/html",
    )
    with pytest.raises(AppError):
        mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - direct ownership invariant.
            cast("EmailMessage", wrong_type), tree.root
        )


def test_stdlib_tree_rejects_count_unvisited_and_header_defect_states() -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nx\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    parsed = mime_stdlib_check.parse_stdlib(raw)
    tree.root.children.clear()
    with pytest.raises(AppError):
        mime_stdlib_check.validate_stdlib_tree(parsed, tree.by_path)
    tree = parse_raw_mime(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    tree.by_path[1,] = tree.root
    with pytest.raises(AppError):
        mime_stdlib_check.validate_stdlib_tree(
            mime_stdlib_check.parse_stdlib(tree.raw), tree.by_path
        )

    class HeaderDefectPart:
        def __iter__(self) -> object:
            return iter(("Content-Type",))

        def __getitem__(self, _name: str) -> object:
            return SimpleNamespace(defects=(ValueError(),))

    with pytest.raises(AppError):
        mime_stdlib_check._validate_headers(  # ruff: ignore[private-member-access] - direct header-defect contract.
            cast("EmailMessage", HeaderDefectPart())
        )

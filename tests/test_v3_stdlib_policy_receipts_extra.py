"""Additional exact receipts for independent MIME checks and compound policy."""

from __future__ import annotations

from email import errors
from email import policy as email_policy
from email.message import EmailMessage
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import mime_stdlib_check
from eml_attachment_remover.domain import (
    AppError,
    DecisionAction,
    ExitCode,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_stdlib_check import StdlibValidationWork
from eml_attachment_remover.mime_validation import ContentSpec

if TYPE_CHECKING:
    from email.policy import EmailPolicy


def test_stdlib_parser_uses_a_separate_no_refold_policy_and_preserves_result(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The independent parser must neither refold source nor substitute its result."""
    seen: list[object] = []
    expected = EmailMessage()

    class RecordingParser:
        def __init__(self, *, policy: object) -> None:
            seen.append(policy)

        @staticmethod
        def parsebytes(raw: bytes) -> EmailMessage:
            assert raw == b"X-Trace: source\r\n\r\nbody\r\n"
            return expected

    monkeypatch.setattr(mime_stdlib_check, "BytesParser", RecordingParser)
    assert (
        mime_stdlib_check.parse_stdlib(b"X-Trace: source\r\n\r\nbody\r\n") is expected
    )
    assert len(seen) == 1
    parser_policy = cast("EmailPolicy", seen[0])
    assert parser_policy is not email_policy.default
    assert parser_policy.refold_source == "none"


@pytest.mark.parametrize(
    "failure",
    [
        errors.MessageParseError("malformed"),
        RecursionError("too deep"),
        ValueError("invalid parser state"),
    ],
)
def test_stdlib_parser_wraps_every_expected_parser_failure_exactly(
    monkeypatch: pytest.MonkeyPatch, failure: BaseException
) -> None:
    """Every documented CPython parse failure has one stable public receipt."""

    class FailingParser:
        def __init__(self, *, policy: object) -> None:
            del policy

        @staticmethod
        def parsebytes(_raw: bytes) -> EmailMessage:
            raise failure

    monkeypatch.setattr(mime_stdlib_check, "BytesParser", FailingParser)
    with pytest.raises(AppError) as raised:
        mime_stdlib_check.parse_stdlib(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "stdlib MIME parser rejected source"
    )
    assert raised.value.__cause__ is failure


def test_stdlib_tree_reports_all_nonopaque_nodes_and_edges_exactly() -> None:
    """Nested ordinary MIME branches must each receive one independent comparison."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=outer\r\nX-Root: one\r\n\r\n"
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


def test_stdlib_ownership_requires_exact_duplicate_name_sequence_and_type() -> None:
    """Ownership includes every physical field occurrence, but never field values."""
    node = RawNode(
        (),
        0,
        0,
        0,
        (
            Header(b"content-type", b"text/plain", 0, 26),
            Header(b"x-trace", b"one", 26, 40),
            Header(b"x-trace", b"two", 40, 54),
        ),
        ContentSpec("text/plain", {}),
        None,
        "7bit",
    )
    matching = cast(
        "EmailMessage",
        SimpleNamespace(
            raw_items=lambda: iter((
                ("CONTENT-TYPE", "not compared"),
                ("X-Trace", "first changed value"),
                ("x-TRACE", "second changed value"),
            )),
            get_content_type=lambda: "TEXT/PLAIN",
        ),
    )
    mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - exact raw/stdlib ownership contract.
        matching, node
    )
    missing_duplicate = cast(
        "EmailMessage",
        SimpleNamespace(
            raw_items=lambda: (("Content-Type", "text/plain"), ("X-Trace", "one")),
            get_content_type=lambda: "text/plain",
        ),
    )
    with pytest.raises(AppError) as ownership_error:
        mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - duplicate physical ownership is observable.
            missing_duplicate, node
        )
    assert ownership_error.value == AppError(
        ExitCode.PARSE_ERROR, "raw MIME header ownership disagrees with stdlib"
    )
    wrong_type = cast(
        "EmailMessage",
        SimpleNamespace(
            raw_items=lambda: (
                ("Content-Type", "text/plain"),
                ("X-Trace", "one"),
                ("X-Trace", "two"),
            ),
            get_content_type=lambda: "text/html",
        ),
    )
    with pytest.raises(AppError) as type_error:
        mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - media-type ownership is independent of fields.
            wrong_type, node
        )
    assert type_error.value == AppError(
        ExitCode.PARSE_ERROR, "raw MIME media type disagrees with stdlib"
    )


def test_mixed_and_related_compounds_produce_one_complete_nested_plan() -> None:
    """Mixed delegates retained compounds while pruning every direct attachment."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--m\r\nContent-Type: multipart/related; boundary=r; "
        b'type="multipart/alternative"; start="<root@x>"; '
        b'start-info="<plain@x> <html@x>"\r\n\r\n'
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <before@x>\r\n\r\nbefore\r\n"
        b"--r\r\nContent-Type: multipart/alternative; boundary=a\r\n"
        b"Content-ID: <root@x>\r\n\r\n"
        b"--a\r\nContent-Type: text/plain\r\nContent-ID: <plain@x>\r\n\r\n"
        b"plain\r\n--a\r\nContent-Type: text/html\r\nContent-ID: <html@x>\r\n\r\n"
        b"<html/>\r\n--a--\r\n"
        b"--r\r\nContent-Type: image/gif\r\nContent-ID: <after@x>\r\n\r\nafter\r\n"
        b"--r--\r\n"
        b"--m\r\nContent-Type: application/pkcs7-mime\r\n"
        b"Content-Disposition: attachment\r\n\r\nopaque attachment\r\n--m--\r\n"
    )
    plan = classify(parse_raw_mime(raw).root)
    assert plan.actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.KEEP,
        (1,): DecisionAction.RECURSE,
        (1, 0): DecisionAction.REMOVE_SUBTREE,
        (1, 1): DecisionAction.RECURSE,
        (1, 1, 0): DecisionAction.KEEP,
        (1, 1, 1): DecisionAction.KEEP,
        (1, 2): DecisionAction.REMOVE_SUBTREE,
        (2,): DecisionAction.REMOVE_SUBTREE,
    }
    assert plan.removals == (
        Removal((1, 0), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
        Removal((1, 2), "image/gif", RemovalReason.RELATED_NONROOT_COMPONENT),
        Removal((2,), "application/pkcs7-mime", RemovalReason.EXPLICIT_ATTACHMENT),
    )


@pytest.mark.parametrize(
    ("raw", "message"),
    [
        (
            (
                b"Content-Type: multipart/mixed; boundary=m\r\n"
                b"Content-Disposition: attachment\r\n\r\n"
                b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m--\r\n"
            ),
            "attachment mixed role conflict",
        ),
        (
            (
                b"Content-Type: multipart/related; boundary=r\r\n"
                b"Content-Disposition: attachment\r\n\r\n"
                b"--r\r\nContent-Type: text/plain\r\n\r\nbody\r\n--r--\r\n"
            ),
            "attachment related role conflict",
        ),
    ],
)
def test_attachment_compound_roles_cannot_authorize_mixed_or_related_plans(
    raw: bytes, message: str
) -> None:
    """Only a mixed child attachment is removable; compound roots remain body roles."""
    with pytest.raises(AppError) as raised:
        classify(parse_raw_mime(raw).root)
    assert raised.value == AppError(ExitCode.TRANSFORMATION_UNAVAILABLE, message, ())

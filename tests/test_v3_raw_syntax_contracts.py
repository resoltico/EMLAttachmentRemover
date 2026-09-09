"""Synthetic syntax contracts for headers, identifiers, and MIME parameters."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_headers, mime_identifiers, mime_validation
from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_headers import Header


@pytest.mark.parametrize(
    "raw",
    [
        b" From: orphan\r\n",
        b"bad header\r\n",
        b"Content-Type: text/plain\r\nContent-Type: text/html\r\n",
    ],
)
def test_header_parser_rejects_orphan_malformed_and_duplicate_controls(
    raw: bytes,
) -> None:
    with pytest.raises(AppError):
        mime_headers.parse_headers(raw, 0, len(raw))


def test_header_parser_preserves_mbox_and_folded_fields(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    raw = b"From sender\r\nSubject: first\r\n\tsecond\r\n"
    headers = mime_headers.parse_headers(raw, 0, len(raw))
    assert headers[0].name == b"subject"
    assert b"second" in headers[0].value
    assert (
        mime_headers.parse_headers(b"\r\nSubject: one\r\n", 0, 16)[0].name == b"subject"
    )
    with monkeypatch.context() as context:
        context.setattr(mime_headers, "MAX_HEADERS", 0)
        with pytest.raises(AppError):
            mime_headers.parse_headers(b"Subject: one\r\n", 0, 14)
    with monkeypatch.context() as context:
        context.setattr(mime_headers, "MAX_HEADER_BYTES", 0)
        with pytest.raises(AppError):
            mime_headers.parse_headers(b"Subject: one\r\n", 0, 14)


@pytest.mark.parametrize(
    "value",
    [b"(nested (ok)) <id@x>", b"(escaped\\) comment) <id@x>", b" <id@x> "],
)
def test_identifier_parser_accepts_exact_cfws_wrappers(value: bytes) -> None:
    assert mime_identifiers.parse_message_identifier(value, field="test") == b"id@x"


@pytest.mark.parametrize(
    "value",
    [b"(unterminated <id@x>", b"(line\n) <id@x>", b"id@x", b"<id", b"<>"],
)
def test_identifier_parser_rejects_malformed_comments_and_angles(value: bytes) -> None:
    with pytest.raises(AppError):
        mime_identifiers.parse_message_identifier(value, field="test")


@pytest.mark.parametrize("value", [b"<a@x><b@x>", b"", b"<a@x> trailing"])
def test_identifier_sequence_requires_cfws_and_no_trailing_text(value: bytes) -> None:
    with pytest.raises(AppError):
        mime_identifiers.parse_message_identifier_sequence(value, field="test")


def test_identifier_parser_rejects_trailing_non_cfws_text() -> None:
    with pytest.raises(AppError):
        mime_identifiers.parse_message_identifier(b"<a@x>tail", field="test")


@pytest.mark.parametrize(
    "value",
    [
        b"text/plain; broken",
        b'text/plain; x="unterminated',
        b"text/plain; x*=utf-8''bad space",
        b"text/plain; x*1=value",
        b"text/plain; x=one; x=two",
        b"text/plain; x*0=one; x*0=two",
        b"text/plain; x*0=one; x*2=two",
    ],
)
def test_structured_content_type_rejects_invalid_parameter_forms(value: bytes) -> None:
    headers = (Header(b"content-type", value, 0, len(value)),)
    with pytest.raises(AppError):
        mime_validation.content_specs(headers)


def test_content_specs_defaults_and_rejects_bad_cte_and_content_id() -> None:
    assert mime_validation.content_specs(())[-1] == "7bit"
    for name, value in [
        (b"content-transfer-encoding", b"bad cte"),
        (b"content-id", b"bad"),
    ]:
        with pytest.raises(AppError):
            mime_validation.content_specs((Header(name, value, 0, len(value)),))

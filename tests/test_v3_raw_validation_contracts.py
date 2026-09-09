"""Synthetic parser contracts for closed MIME parameter grammar."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError


@pytest.mark.parametrize(
    ("value", "count"),
    [(b'one; "two;three"; four', 3), (b'one; escaped="a\\"b"', 2)],
)
def test_semicolon_splitter_preserves_quoted_regions(value: bytes, count: int) -> None:
    assert len(mime_validation._split_semicolons(value)) == count  # ruff: ignore[private-member-access] - direct structured-header tokenizer.


@pytest.mark.parametrize(
    "value",
    [b'"unterminated', b'"trailing\\"', b'"quoted\\"'],
)
def test_quoted_value_parser_rejects_unfinished_forms(value: bytes) -> None:
    with pytest.raises(AppError):
        mime_validation._unquote(value)  # ruff: ignore[private-member-access] - direct quoted-pair invariant.


def test_quoted_value_parser_preserves_one_valid_quoted_pair() -> None:
    assert mime_validation._unquote(b'"a\\b"') == b"ab"  # ruff: ignore[private-member-access] - direct quoted-pair contract.


@pytest.mark.parametrize(
    "name",
    [b"", b"bad space", b"name*bad", b"name*x"],
)
def test_parameter_name_parser_rejects_invalid_extensions(name: bytes) -> None:
    with pytest.raises(AppError):
        mime_validation._parameter_name(name)  # ruff: ignore[private-member-access] - direct RFC2231 name invariant.


@pytest.mark.parametrize(
    "case",
    [(b"utf-8'bad", True), (b"utf-8''bad%Z0", True), (b"bad space", False)],
)
def test_extended_parameter_parser_rejects_invalid_payloads(
    case: tuple[bytes, bool],
) -> None:
    value, initial = case
    with pytest.raises(AppError):
        mime_validation._extended_parameter(value, initial=initial)  # ruff: ignore[private-member-access] - direct RFC2231 payload invariant.


def test_extended_parameter_parser_accepts_escape_and_rejects_language_space() -> None:
    assert (
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - percent-escape contract.
            b"utf-8''%41", initial=True
        )
        == b"utf-8''%41"
    )
    with pytest.raises(AppError):
        mime_validation._extended_parameter(b"utf-8'bad space'value", initial=True)  # ruff: ignore[private-member-access] - language-token contract.


def test_structured_helpers_reject_non_media_and_overlapping_parameters() -> None:
    for value in (b"", b"text", b"text//plain"):
        with pytest.raises(AppError):
            mime_validation._media_token(value)  # ruff: ignore[private-member-access] - media token invariant.
    with pytest.raises(AppError):
        mime_validation._parameter_value(b'"quoted"', encoded=True, initial=True)  # ruff: ignore[private-member-access] - encoded quote invariant.
    parameters: dict[bytes, bytes] = {b"x": b"one"}
    with pytest.raises(AppError):
        mime_validation._store_parameter(parameters, {}, b"x", 0, b"two")  # ruff: ignore[private-member-access] - overlap invariant.
    with pytest.raises(AppError):
        mime_validation._ascii_token(b"\xff")  # ruff: ignore[private-member-access] - ASCII token invariant.

"""Exact raw-value contracts for closed MIME validation."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def test_extended_parameters_preserve_the_raw_prefix_and_payload() -> None:
    """RFC 2231 validation must not decode or discard retained wire bytes."""
    initial = b"utf-8'en'annual%20report"
    assert mime_validation._extended_payload(initial) == b"annual%20report"  # ruff: ignore[private-member-access] - exact RFC 2231 payload boundary.
    assert (
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - exact initial extension receipt.
            initial, initial=True
        )
        == initial
    )
    assert (
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - exact continuation extension receipt.
            b"%20final", initial=False
        )
        == b"%20final"
    )


@pytest.mark.parametrize(
    "case",
    [
        (b"utf 8''value", True, "malformed RFC 2231 extended parameter"),
        (b"utf-8'bad space'value", True, "malformed RFC 2231 language"),
        (b"utf-8''bad%2", True, "malformed RFC 2231 escape"),
        (b"utf-8''bad%XZ", True, "malformed RFC 2231 escape"),
        (b"not allowed", False, "malformed RFC 2231 parameter"),
    ],
)
def test_extended_parameters_reject_each_grammar_boundary(
    case: tuple[bytes, bool, str],
) -> None:
    """Every RFC 2231 grammar failure carries the stable parse classification."""
    value, initial, message = case
    with pytest.raises(AppError, match=message) as error:
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - direct RFC 2231 grammar receipt.
            value, initial=initial
        )
    assert error.value.code is ExitCode.PARSE_ERROR


def test_parameter_name_identifies_plain_extended_and_continued_forms() -> None:
    """The continuation index and extended bit are independent raw-wire facts."""
    assert mime_validation._parameter_name(b"Name") == (b"Name", None, False)  # ruff: ignore[private-member-access] - direct parameter-name receipt.
    assert mime_validation._parameter_name(b"Name*") == (b"Name", None, True)  # ruff: ignore[private-member-access] - direct extended-name receipt.
    assert mime_validation._parameter_name(b"Name*19") == (b"Name", 19, False)  # ruff: ignore[private-member-access] - direct continuation-name receipt.
    assert mime_validation._parameter_name(b"Name*19*") == (b"Name", 19, True)  # ruff: ignore[private-member-access] - direct extended-continuation receipt.


@pytest.mark.parametrize(
    ("name", "message"),
    [
        (b"*0", "malformed MIME parameter name"),
        (b"bad space", "malformed MIME parameter name"),
        (b"name*one", "malformed MIME parameter extension"),
        (b"name*1x*", "malformed MIME parameter extension"),
    ],
)
def test_parameter_name_rejects_malformed_base_and_extension(
    name: bytes, message: str
) -> None:
    """Malformed names cannot be normalized into policy-visible keys."""
    with pytest.raises(AppError, match=message) as error:
        mime_validation._parameter_name(name)  # ruff: ignore[private-member-access] - direct parameter-name grammar.
    assert error.value.code is ExitCode.PARSE_ERROR


def test_unquote_preserves_unquoted_and_decodes_every_quoted_pair_once() -> None:
    """Quoted MIME values have exact byte semantics, including escaped quote marks."""
    assert mime_validation._unquote(b"token") == b"token"  # ruff: ignore[private-member-access] - direct unquoted-value receipt.
    assert mime_validation._unquote(b'"a\\;b\\"c"') == b'a;b"c'  # ruff: ignore[private-member-access] - direct quoted-pair decoding receipt.
    assert mime_validation._unquote(b'""') == b""  # ruff: ignore[private-member-access] - exact empty quoted value.
    for value, message in [
        (b'"', "malformed MIME quoted parameter"),
        (b'"unterminated', "malformed MIME quoted parameter"),
        (b'"trailing\\"', "unterminated MIME quoted-pair"),
    ]:
        with pytest.raises(AppError, match=message) as error:
            mime_validation._unquote(value)  # ruff: ignore[private-member-access] - direct quoted-string grammar.
        assert error.value.code is ExitCode.PARSE_ERROR


def test_structured_token_and_parameter_values_preserve_their_forms() -> None:
    """Media token normalization and parameter syntaxes remain independently closed."""
    assert (
        mime_validation._structured_token(  # ruff: ignore[private-member-access] - direct media-token receipt.
            b"Application/JSON", media=True
        )
        == b"application/json"
    )
    assert (
        mime_validation._structured_token(  # ruff: ignore[private-member-access] - direct disposition-token receipt.
            b"Attachment", media=False
        )
        == b"attachment"
    )
    assert (
        mime_validation._parameter_value(  # ruff: ignore[private-member-access] - plain token parameter receipt.
            b"atom", encoded=False, initial=False
        )
        == b"atom"
    )
    assert (
        mime_validation._parameter_value(  # ruff: ignore[private-member-access] - quoted parameter receipt.
            b'"a\\;b"', encoded=False, initial=False
        )
        == b"a;b"
    )
    extended = b"utf-8''two%20words"
    assert (
        mime_validation._parameter_value(  # ruff: ignore[private-member-access] - extended parameter receipt.
            extended, encoded=True, initial=True
        )
        == extended
    )
    for value, media, message in [
        (b"text/plain/extra", True, "malformed MIME media type"),
        (b"text/", True, "malformed MIME media type"),
        (b" space", False, "malformed MIME structured header"),
    ]:
        with pytest.raises(AppError, match=message) as error:
            mime_validation._structured_token(  # ruff: ignore[private-member-access] - direct structured-token grammar.
                value, media=media
            )
        assert error.value.code is ExitCode.PARSE_ERROR
    for value, encoded, initial, message in [
        (b"space value", False, False, "malformed MIME parameter value"),
        (b'"quoted"', True, True, "quoted RFC 2231 parameter"),
        (b"bad%2", True, False, "malformed RFC 2231 escape"),
    ]:
        with pytest.raises(AppError, match=message) as error:
            mime_validation._parameter_value(  # ruff: ignore[private-member-access] - direct parameter-value grammar.
                value, encoded=encoded, initial=initial
            )
        assert error.value.code is ExitCode.PARSE_ERROR


def test_parameter_store_records_direct_and_continuation_ownership() -> None:
    """Direct values and numbered pieces retain exclusive ownership of each name."""
    parameters: dict[bytes, bytes] = {}
    continuations: dict[bytes, dict[int, bytes]] = {}
    mime_validation._store_parameter(  # ruff: ignore[private-member-access] - direct parameter ownership receipt.
        parameters, continuations, b"plain", None, b"one"
    )
    mime_validation._store_parameter(  # ruff: ignore[private-member-access] - continuation ownership receipt.
        parameters, continuations, b"joined", 0, b"first"
    )
    mime_validation._store_parameter(  # ruff: ignore[private-member-access] - distinct continuation segment receipt.
        parameters, continuations, b"joined", 2, b"third"
    )
    assert parameters == {b"plain": b"one"}
    assert continuations == {b"joined": {0: b"first", 2: b"third"}}


@pytest.mark.parametrize(
    ("parameters", "continuations", "name", "segment", "message"),
    [
        ({b"name": b"one"}, {}, b"name", None, "duplicate MIME parameter"),
        ({b"name": b"one"}, {}, b"name", 0, "overlapping MIME parameter"),
        ({}, {b"name": {0: b"one"}}, b"name", None, "duplicate MIME parameter"),
        ({}, {b"name": {0: b"one"}}, b"name", 0, "duplicate MIME parameter segment"),
    ],
)
def test_parameter_store_rejects_each_ownership_collision(
    parameters: dict[bytes, bytes],
    continuations: dict[bytes, dict[int, bytes]],
    name: bytes,
    segment: int | None,
    message: str,
) -> None:
    """A collision is rejected without overwriting the previously retained bytes."""
    initial_parameters = parameters.copy()
    initial_continuations = {
        key: entries.copy() for key, entries in continuations.items()
    }
    with pytest.raises(AppError, match=message) as error:
        mime_validation._store_parameter(  # ruff: ignore[private-member-access] - direct ownership conflict.
            parameters, continuations, name, segment, b"replacement"
        )
    assert error.value.code is ExitCode.PARSE_ERROR
    assert parameters == initial_parameters
    assert continuations == initial_continuations


def test_finish_continuations_sorts_then_joins_each_exact_segment() -> None:
    """A complete numbered sequence becomes one exact raw parameter value."""
    parameters = {b"plain": b"retained"}
    continuations = {
        b"filename": {2: b".eml", 0: b"utf-8''mail", 1: b"%20copy"},
    }
    mime_validation._finish_continuations(  # ruff: ignore[private-member-access] - continuation completion receipt.
        parameters, continuations
    )
    assert parameters == {
        b"plain": b"retained",
        b"filename": b"utf-8''mail%20copy.eml",
    }


@pytest.mark.parametrize(
    "continuations",
    [
        {b"filename": {1: b"one"}},
        {b"filename": {0: b"one", 2: b"three"}},
    ],
)
def test_finish_continuations_rejects_every_gapped_sequence(
    continuations: dict[bytes, dict[int, bytes]],
) -> None:
    """A gap cannot silently become a policy-visible filename or media parameter."""
    parameters: dict[bytes, bytes] = {}
    with pytest.raises(AppError, match="gapped MIME parameter continuation") as error:
        mime_validation._finish_continuations(  # ruff: ignore[private-member-access] - continuation gap invariant.
            parameters, continuations
        )
    assert error.value.code is ExitCode.PARSE_ERROR
    assert parameters == {}


def test_structured_field_and_content_specs_return_complete_raw_receipts() -> None:
    """All MIME controls retain their normalized tokens and raw values together."""
    type_value = (
        b"Application/JSON; Charset=US-ASCII; name*0*=utf-8'en'mail%20; name*1*=copy"
    )
    disposition_value = b'Attachment; filename="quarterly; report.eml"; size=42'
    assert mime_validation._structured(  # ruff: ignore[private-member-access] - complete media structured-field receipt.
        type_value, media=True
    ) == ContentSpec(
        "application/json",
        {b"charset": b"US-ASCII", b"name": b"utf-8'en'mail%20copy"},
    )
    assert mime_validation._structured(  # ruff: ignore[private-member-access] - complete disposition structured-field receipt.
        disposition_value, media=False
    ) == ContentSpec(
        "attachment",
        {b"filename": b"quarterly; report.eml", b"size": b"42"},
    )
    headers = (
        Header(b"content-type", type_value, 0, len(type_value)),
        Header(
            b"content-disposition", disposition_value, 1, 1 + len(disposition_value)
        ),
        Header(b"content-transfer-encoding", b" BASE64 ", 2, 10),
        Header(b"content-id", b"<node@example.test>", 3, 22),
    )
    assert mime_validation.content_specs(headers) == (
        ContentSpec(
            "application/json",
            {b"charset": b"US-ASCII", b"name": b"utf-8'en'mail%20copy"},
        ),
        ContentSpec(
            "attachment",
            {b"filename": b"quarterly; report.eml", b"size": b"42"},
        ),
        "base64",
    )


@pytest.mark.parametrize(
    ("headers", "message"),
    [
        (
            (Header(b"content-transfer-encoding", b"base 64", 0, 7),),
            "malformed Content-Transfer-Encoding",
        ),
        (
            (Header(b"content-id", b"<node@example.test> trailing", 0, 27),),
            "malformed Content-ID",
        ),
        (
            (Header(b"content-disposition", b"attachment; bad", 0, 15),),
            "malformed MIME parameter",
        ),
    ],
)
def test_content_specs_rejects_each_outer_control_failure(
    headers: tuple[Header, ...], message: str
) -> None:
    """Bad control fields fail before MIME policy sees an incomplete receipt."""
    with pytest.raises(AppError, match=message) as error:
        mime_validation.content_specs(headers)
    assert error.value.code is ExitCode.PARSE_ERROR

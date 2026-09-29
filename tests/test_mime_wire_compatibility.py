"""Wire spans, RFC 2231 controls, and header folding, against independent bytes."""

from __future__ import annotations

import hashlib
import json
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli, mime_headers, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode

if TYPE_CHECKING:
    from pathlib import Path

HEADERS = b"From: a@example.test\r\nMIME-Version: 1.0\r\n"
ATTACHMENT = (
    b"Content-Type: application/pdf\r\nContent-Disposition: attachment\r\n\r\nX\r\n"
)


def _mixed(parts: bytes, boundary: bytes = b"B", parameter: bytes = b"") -> bytes:
    """Build a multipart/mixed message whose part bytes are written by hand.

    Returns:
        The complete message.

    """
    declared = parameter or b"boundary=" + boundary
    return (
        HEADERS
        + b"Content-Type: multipart/mixed; "
        + declared
        + b"\r\n\r\n"
        + parts
        + b"--"
        + boundary
        + b"--\r\n"
    )


def _child_payloads(raw: bytes) -> list[bytes]:
    """Return each child's body octets, without the line break its delimiter owns.

    Returns:
        The bodies, in order.

    """
    tree = mime_raw.parse_raw_mime(raw)
    return [
        raw[child.body_start : child.end].removesuffix(b"\r\n")
        for child in tree.root.children
    ]


def _dry_run(
    source: Path, capsys: pytest.CaptureFixture[str]
) -> tuple[int, dict[str, object]]:
    status = cli.main(["--dry-run", "--output-format", "json", "--", str(source)])
    return status, json.loads(capsys.readouterr().out)


@pytest.mark.parametrize(
    ("ending", "body"),
    [
        (b"\r\n", b"plain body"),
        (b"\n", b"plain body"),
        (b"\r", b"plain body"),
        # Only the first line ending is the separator; later blank lines are body.
        (b"\r\n", b"\r\nplain body"),
        (b"\r\n", b"\r\n\r\nplain body\r\n"),
    ],
)
def test_empty_header_block_consumes_exactly_its_separator(
    ending: bytes, body: bytes
) -> None:
    """A part with no header fields owns only the one separator line (finding 5)."""
    raw = _mixed(b"--B" + ending + ending + body + b"\r\n" + b"--B\r\n" + ATTACHMENT)
    assert _child_payloads(raw)[0] == body


def test_headerless_body_without_a_separator_is_kept_whole() -> None:
    """A body that starts at once, with no separator line, loses no byte."""
    raw = _mixed(b"--B\r\nfree text without fields\r\n--B\r\n" + ATTACHMENT)
    assert _child_payloads(raw)[0] == b"free text without fields"


def test_reported_payload_hash_describes_only_the_payload(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The retained fingerprint hashes the body octets, not the separator."""
    body = b"hello body"
    source = tmp_path / "m.eml"
    source.write_bytes(_mixed(b"--B\r\n\r\n" + body + b"\r\n--B\r\n" + ATTACHMENT))
    status, report = _dry_run(source, capsys)
    retained = report["items"][0]["transformation"]["retained"][0]  # type: ignore[index]
    assert status == 0
    assert retained["encoded_sha256"] == hashlib.sha256(body).hexdigest()
    assert retained["decoded_sha256"] == hashlib.sha256(body).hexdigest()


def test_root_of_complete_fields_is_a_message_with_an_empty_body() -> None:
    """RFC 5322 makes the body optional: fields alone are a whole message."""
    raw = (
        b"From: a@example.test\r\nDate: Mon, 1 Jan 2024 00:00:00 +0000\r\n"
        b"Subject: folded\r\n across two lines\r\n"
    )
    root = mime_raw.parse_raw_mime(raw).root
    assert (root.body_start, root.end, root.children) == (len(raw), len(raw), [])
    assert [header.name for header in root.headers] == [b"from", b"date", b"subject"]


def test_fields_only_message_is_accepted_end_to_end(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A complete From/Date/Subject message needs no separator to be processed."""
    source = tmp_path / "fields.eml"
    source.write_bytes(b"From: a@example.test\r\nSubject: s\r\n")
    status, report = _dry_run(source, capsys)
    assert (status, report["items"][0]["status"]) == (0, "would_create")  # type: ignore[index]


@pytest.mark.parametrize(
    "raw",
    [
        # A malformed line is not a complete field, so the separator is missing.
        b"From: a@example.test\r\nnot a field\r\n",
        # A header-like nested part still needs its separator.
        _mixed(b"--B\r\nContent-Type: text/plain\r\n"),
    ],
)
def test_missing_separator_still_fails_closed(raw: bytes) -> None:
    """Only a complete root message may omit its separator."""
    with pytest.raises(AppError, match="lacks a body separator") as raised:
        mime_raw.parse_raw_mime(raw)
    assert raised.value.code is ExitCode.PARSE_ERROR


@pytest.mark.parametrize(
    ("parameter", "boundary"),
    [
        (b"boundary*=us-ascii''ABC", b"ABC"),
        (b"boundary*=''ABC", b"ABC"),
        (b"boundary*=us-ascii'en'A%42C", b"ABC"),
        (b"boundary*0*=us-ascii''A; boundary*1*=%42; boundary*2*=C", b"ABC"),
        # An unencoded continuation is literal, so its percent sign is a boundary byte.
        (b"boundary*0*=us-ascii''A; boundary*1=%42", b"A%42"),
        # An encoded later segment is decoded on its own; the unencoded first is not.
        (b"boundary*0=A; boundary*1*=%42", b"AB"),
    ],
)
def test_extended_boundary_finds_its_delimiters(
    parameter: bytes, boundary: bytes
) -> None:
    """Encoded boundary forms locate the real delimiters (finding 6)."""
    raw = _mixed(
        b"--" + boundary + b"\r\nContent-Type: text/plain\r\n\r\nkept\r\n"
        b"--" + boundary + b"\r\n" + ATTACHMENT,
        boundary,
        parameter,
    )
    assert _child_payloads(raw) == [b"kept", b"X"]


def test_extended_values_keep_their_wire_spelling_and_their_meaning() -> None:
    """Evidence stays exact wire text; only structural lookups use the decoding."""
    raw = _mixed(b"--B\r\n\r\nx\r\n--B\r\n" + ATTACHMENT, b"B", b"boundary*=''%42")
    root = mime_raw.parse_raw_mime(raw).root
    assert root.content_type.parameters[b"boundary"] == b"''%42"
    assert root.content_type.decoded[b"boundary"] == b"B"
    assert root.parameter(b"boundary") == b"B"


@pytest.mark.parametrize(
    "parameter",
    [
        b"boundary*=utf-8''%C3%A9",
        b"boundary*=''%00",
        b"boundary*=''A%1FB",
        b"boundary*=''%7F",
    ],
)
def test_decoded_structural_controls_must_be_printable_ascii(parameter: bytes) -> None:
    """An encoded control cannot smuggle non-ASCII or control octets."""
    raw = _mixed(b"--B\r\n\r\nx\r\n", b"B", parameter)
    with pytest.raises(AppError, match="not printable ASCII") as raised:
        mime_raw.parse_raw_mime(raw)
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR,
        "RFC 2231 structural MIME parameter is not printable ASCII",
        (),
    )


@pytest.mark.parametrize("boundary", [b"A B", b"A~", b"~A", b"A!"])
def test_the_whole_printable_ascii_range_is_a_valid_decoded_boundary(
    boundary: bytes,
) -> None:
    """Space (0x20) through tilde (0x7E) are inclusive bounds of the range."""
    encoded = b"boundary*=''" + boundary.replace(b" ", b"%20").replace(b"~", b"%7E")
    raw = _mixed(
        b"--" + boundary + b"\r\n\r\nx\r\n--" + boundary + b"\r\n" + ATTACHMENT,
        boundary,
        encoded,
    )
    assert _child_payloads(raw)[0] == b"x"


@pytest.mark.parametrize(
    "parameter",
    [b"filename*=''a%20b.pdf", b"filename*=us-ascii''a%20b.pdf", b"filename*='en'a"],
)
def test_empty_charset_is_valid_and_the_attachment_is_still_removed(
    parameter: bytes, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """RFC 2231 allows an empty charset field (finding 6)."""
    source = tmp_path / "m.eml"
    source.write_bytes(
        _mixed(
            b"--B\r\nContent-Type: text/plain\r\n\r\nkept\r\n--B\r\n"
            b"Content-Type: application/pdf\r\nContent-Disposition: attachment; "
            + parameter
            + b"\r\n\r\nX\r\n"
        )
    )
    status, report = _dry_run(source, capsys)
    item = report["items"][0]  # type: ignore[index]
    assert (status, item["status"]) == (0, "would_create")
    assert item["transformation"]["removal_roots"][0]["reason"] == "EXPLICIT_ATTACHMENT"


@pytest.mark.parametrize(
    "parameter", [b"filename*=%41", b"filename*='a", b"filename*=bad space''a"]
)
def test_malformed_extended_prefix_still_fails_closed(parameter: bytes) -> None:
    """Both apostrophes stay mandatory and the charset stays a token."""
    raw = _mixed(
        b"--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; "
        + parameter
        + b"\r\n\r\nX\r\n"
    )
    with pytest.raises(AppError, match="malformed RFC 2231"):
        mime_raw.parse_raw_mime(raw)


def test_folded_headers_are_joined_once_from_their_physical_lines() -> None:
    """Each continuation is collected, then joined once with its original CRLF."""
    raw = b"X-Long: first\r\n second\r\n\tthird\r\nX-Next: after\r\n\r\n"
    separator = raw.index(b"\r\n\r\n") + 2
    long, following = mime_headers.parse_headers(raw, 0, separator)
    assert long.value == b"first\r\n second\r\n\tthird"
    assert (long.start, long.end) == (0, raw.index(b"X-Next"))
    assert following == mime_headers.Header(
        b"x-next", b"after", raw.index(b"X-Next"), separator
    )


def test_a_field_is_not_rebuilt_for_each_continuation_line() -> None:
    """The accumulated prefix is never copied per line (finding 10)."""
    fields: list[mime_headers._Field] = []
    mime_headers._append_line(fields, b"X-Long: start", 0, 14)  # ruff: ignore[private-member-access] - accumulation architecture
    first = fields[0].first
    for line_number in range(1, 50):
        mime_headers._append_line(  # ruff: ignore[private-member-access] - accumulation architecture
            fields, b" more", 14 * line_number, 14 * line_number + 7
        )
    # Only the small record tracks the running end; the value is never re-copied.
    assert fields[0].first.value is first.value
    assert (fields[0].first.start, fields[0].first.end) == (0, 14 * 49 + 7)
    assert len(fields[0].continuations) == 49
    assert fields[0].header().value == b"start" + b"\r\n more" * 49

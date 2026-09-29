"""The faster delimiter scan and text checks give the answers the slow ones gave.

Each optimized function is held against a literal copy of the implementation it
replaced, on generated inputs and on the cases where the two could differ.
"""

from __future__ import annotations

import unicodedata

import pytest
from hypothesis import given
from hypothesis import strategies as st

from eml_attachment_remover import mime_raw, native_values
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import line_end

Span = tuple[int, int, bool]
ESCAPED_CATEGORIES = {"Cc", "Cf", "Cs", "Zl", "Zp"}
SURROGATE_FIRST = 0xD800
SURROGATE_LAST = 0xDFFF
UNICODE_LIMIT = 0x110000
PAYLOAD_LINES = 200_000
# Written as code points: a formatter may rewrite an escape into the invisible or
# combined character itself, and a lone surrogate cannot be written as a literal.
LINE_SEPARATOR = chr(0x2028)
PARAGRAPH_SEPARATOR = chr(0x2029)
ZERO_WIDTH_SPACE = chr(0x200B)
NO_BREAK_SPACE = chr(0x00A0)
PRIVATE_USE = chr(0xE000)
LAST_CODE_POINT = chr(0x10FFFF)
HIGH_SURROGATE = chr(0xD800)
LOW_SURROGATE = chr(0xDFFF)
INNER_SURROGATE = chr(0xDC80)
LEAD_SURROGATE = chr(0xD83D)
TRAIL_SURROGATE = chr(0xDE00)
BELOW_SURROGATES = chr(0xD7FF)
WIDE_TEXT = "wide " + chr(0xE9) + chr(0x6587) + " " + chr(0x1F600)


def _reference_delimiter_lines(
    raw: bytes, start: int, end: int, boundary: bytes
) -> list[Span]:
    """Return what a walk over every wire line finds, as the scanner once did.

    Returns:
        The delimiter spans a walk over every line records.

    Raises:
        AppError: If the line cursor stops advancing.

    """
    prefix = b"--" + boundary
    result: list[Span] = []
    position = start
    while position < end:
        end_of_line = line_end(raw, position, end)
        if end_of_line <= position:
            message = "MIME delimiter cursor did not advance"
            raise AppError(ExitCode.PARSE_ERROR, message)
        line = raw[position:end_of_line].rstrip(b"\r\n")
        if line.startswith(prefix):
            tail = line[len(prefix) :]
            closing = tail.startswith(b"--")
            if closing:
                tail = tail[2:]
            if not tail.strip(b" \t"):
                result.append((position, end_of_line, closing))
        position = end_of_line
    return result


def _reference_safe_display(value: str) -> str:
    """Return the per-character escape as it was written before the fast path.

    Returns:
        The text with each escaped category replaced by its four-digit escape.

    """
    return "".join(
        character
        if unicodedata.category(character) not in ESCAPED_CATEGORIES
        else f"\\u{ord(character):04x}"
        for character in value
    )


def _reference_has_surrogate(text: str) -> bool:
    """Return the per-character surrogate test as it was written before.

    Returns:
        Whether any character lies in the surrogate range.

    """
    return any(
        SURROGATE_FIRST <= ord(character) <= SURROGATE_LAST for character in text
    )


# Pieces chosen so that delimiter text, every line break, near misses, and text that
# only looks like a delimiter mid-line all occur, in every order.
WIRE_PIECES = st.sampled_from([
    b"--b",
    b"--b--",
    b"--bound",
    b"--bound--",
    b"-",
    b"--",
    b"b",
    b"x",
    b" ",
    b"\t",
    b"\v",
    b"\r\n",
    b"\n",
    b"\r",
    b"\r\r",
    b"payload line",
])
BOUNDARIES = st.sampled_from([b"b", b"bound", b"b\n", b"\r", b"b\r\n", b"--", b"x y"])
ANY_TEXT = st.text(alphabet=st.characters(codec=None, exclude_categories=()))


@st.composite
def _scan_case(draw: st.DrawFn) -> tuple[bytes, int, int, bytes]:
    """Draw wire bytes with a boundary and an arbitrary, possibly empty, range.

    Returns:
        The wire bytes, the range start and end, and the boundary.

    """
    raw = b"".join(draw(st.lists(WIRE_PIECES, max_size=40)))
    start = draw(st.integers(min_value=0, max_value=len(raw)))
    end = draw(st.integers(min_value=start, max_value=len(raw)))
    return raw, start, end, draw(BOUNDARIES)


@given(_scan_case())
def test_scan_matches_the_walk_over_every_line(  # type: ignore[misc]
    case: tuple[bytes, int, int, bytes],
) -> None:
    """Every range of every wire mixture yields the spans a line walk yields."""
    raw, start, end, boundary = case
    scanned = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - differential check of the scanner.
        raw, start, end, boundary
    )
    assert scanned == _reference_delimiter_lines(raw, start, end, boundary)


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        pytest.param(b"x --b\r\n--b--\r\n", [(7, 14, True)], id="mid-line text"),
        pytest.param(b"a\n--b\nc", [(2, 6, False)], id="after LF"),
        pytest.param(b"a\r--b\rc", [(2, 6, False)], id="after a lone CR"),
        pytest.param(b"a\r\n--b\r\nc", [(3, 8, False)], id="after CRLF"),
        pytest.param(b"--b", [(0, 3, False)], id="unterminated final line"),
        pytest.param(b"--b--", [(0, 5, True)], id="closing without a line break"),
        pytest.param(
            b"--b\r\n--b\r\n--b--",
            [(0, 5, False), (5, 10, False), (10, 15, True)],
            id="consecutive",
        ),
        pytest.param(b"\r\n--b-- \t\r\n", [(2, 11, True)], id="transport space"),
        pytest.param(b"--bx\r\n--b-\r\n", [], id="near misses stay payload"),
    ],
)
def test_scan_positions_and_kinds(raw: bytes, expected: list[Span]) -> None:
    """Named layouts pin where a delimiter may begin and what it consists of."""
    scanned = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - exact span receipts.
        raw, 0, len(raw), b"b"
    )
    assert scanned == expected
    assert scanned == _reference_delimiter_lines(raw, 0, len(raw), b"b")


def test_the_range_start_begins_a_line_and_the_range_end_truncates_one() -> None:
    """An offset start counts as a line start; a delimiter cut by the end is not one."""
    raw = b"xx--b\r\n--b--\r\n"
    from_offset = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - range-start receipt.
        raw, 2, len(raw), b"b"
    )
    assert from_offset == [(2, 7, False), (7, 14, True)]
    truncated = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - range-end receipt.
        raw, 0, 4, b"b"
    )
    assert truncated == []


@pytest.mark.parametrize("boundary", [b"b\n", b"\r", b"b\r\n"])
def test_a_boundary_containing_a_line_break_never_matches(boundary: bytes) -> None:
    """No single wire line can hold a line break, so such a boundary finds nothing."""
    raw = b"--b\n--b\r\n--\r\n--" + boundary + b"\r\n"
    scanned = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - impossible-boundary receipt.
        raw, 0, len(raw), boundary
    )
    assert scanned == []


def test_a_message_of_many_payload_lines_is_scanned_without_visiting_them() -> None:
    """Payload is skipped at search speed, which is the point of the scan."""
    payload = b"A" * 64 + b"\r\n"
    body = payload * PAYLOAD_LINES
    raw = b"--b\r\n" + body + b"--b--\r\n"
    scanned = mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - bulk payload receipt.
        raw, 0, len(raw), b"b"
    )
    assert scanned == [(0, 5, False), (5 + len(body), len(raw), True)]


@given(ANY_TEXT)
def test_safe_display_matches_the_per_character_escape(text: str) -> None:  # type: ignore[misc]
    """Any text, surrogates and separators included, escapes exactly as before."""
    assert native_values.safe_display(text) == _reference_safe_display(text)


@pytest.mark.parametrize(
    "text",
    [
        "",
        "plain/path/name.eml",
        "tab\there",
        "line\nbreak",
        "\x7f",
        "a" + LINE_SEPARATOR + "b",
        "a" + PARAGRAPH_SEPARATOR + "b",
        ZERO_WIDTH_SPACE + "zero-width",
        HIGH_SURROGATE,
        WIDE_TEXT,
        NO_BREAK_SPACE + "no-break",
        PRIVATE_USE + "private",
        LAST_CODE_POINT,
    ],
)
def test_safe_display_named_cases(text: str) -> None:
    """Printable text passes through; everything else escapes as it always did."""
    assert native_values.safe_display(text) == _reference_safe_display(text)


def test_printable_text_never_contains_an_escaped_category() -> None:
    """The fast path holds only if this is true for every code point, so check all."""
    for code_point in range(UNICODE_LIMIT):
        character = chr(code_point)
        if character.isprintable():
            assert unicodedata.category(character) not in ESCAPED_CATEGORIES, code_point


@given(ANY_TEXT)
def test_surrogate_detection_matches_the_range_test(text: str) -> None:  # type: ignore[misc]
    """Strict UTF-8 refusing to encode is the same as holding a surrogate."""
    detected = native_values.has_surrogate(text)
    assert detected is _reference_has_surrogate(text)


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        ("", False),
        ("ascii only", False),
        (WIDE_TEXT, False),
        (HIGH_SURROGATE, True),
        (LOW_SURROGATE, True),
        ("ok" + INNER_SURROGATE + "ok", True),
        (LEAD_SURROGATE + TRAIL_SURROGATE, True),
        (BELOW_SURROGATES + PRIVATE_USE, False),
    ],
)
def test_surrogate_detection_named_cases(text: str, *, expected: bool) -> None:
    """The boundaries of the surrogate range, and pairs that are not combined."""
    detected = native_values.has_surrogate(text)
    assert detected is expected

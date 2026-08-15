"""Extract actual resource values from decoded CSS source."""

from __future__ import annotations

import re
from typing import Final

CSS_COMMENT_PATTERN: Final = r"/\*[\s\S]*?\*/"
CSS_IDENTIFIER_GAP_PATTERN: Final = rf"(?:{CSS_COMMENT_PATTERN})*"
CSS_U_PATTERN: Final = r"(?:u|\\(?:0{0,4}(?:75|55)[ \t\r\n\f]?|u))"
CSS_I_PATTERN: Final = r"(?:i|\\(?:0{0,4}(?:69|49)[ \t\r\n\f]?|i))"
CSS_M_PATTERN: Final = r"(?:m|\\(?:0{0,4}(?:6d|4d)[ \t\r\n\f]?|m))"
CSS_P_PATTERN: Final = r"(?:p|\\(?:0{0,4}(?:70|50)[ \t\r\n\f]?|p))"
CSS_O_PATTERN: Final = r"(?:o|\\(?:0{0,4}(?:6f|4f)[ \t\r\n\f]?|o))"
CSS_R_PATTERN: Final = r"(?:r|\\(?:0{0,4}(?:72|52)[ \t\r\n\f]?|r))"
CSS_T_PATTERN: Final = r"(?:t|\\(?:0{0,4}(?:74|54)[ \t\r\n\f]?|t))"
CSS_L_PATTERN: Final = r"(?:l|\\(?:0{0,4}(?:6c|4c)[ \t\r\n\f]?|l))"
CSS_URL_IDENTIFIER_PATTERN: Final = CSS_IDENTIFIER_GAP_PATTERN.join((
    CSS_U_PATTERN,
    CSS_R_PATTERN,
    CSS_L_PATTERN,
))
CSS_IMPORT_IDENTIFIER_PATTERN: Final = (
    CSS_I_PATTERN
    + CSS_IDENTIFIER_GAP_PATTERN
    + CSS_M_PATTERN
    + CSS_IDENTIFIER_GAP_PATTERN
    + CSS_P_PATTERN
    + CSS_IDENTIFIER_GAP_PATTERN
    + CSS_O_PATTERN
    + CSS_IDENTIFIER_GAP_PATTERN
    + CSS_R_PATTERN
    + CSS_IDENTIFIER_GAP_PATTERN
    + CSS_T_PATTERN
)
CSS_GAP_PATTERN: Final = rf"(?:\s|{CSS_COMMENT_PATTERN})*"
CSS_URL_PATTERN: Final = re.compile(
    rf"(?is)(?<![\w-]){CSS_URL_IDENTIFIER_PATTERN}{CSS_IDENTIFIER_GAP_PATTERN}"
    r"\(\s*(?:\"((?:\\.|[^\"\\])*)\"|"
    r"'((?:\\.|[^'\\])*)'|((?:\\(?:\r\n|[\s\S])|[^\\)])*))\s*\)",
)
CSS_IMPORT_PATTERN: Final = re.compile(
    rf"(?is)@{CSS_IDENTIFIER_GAP_PATTERN}{CSS_IMPORT_IDENTIFIER_PATTERN}"
    rf"(?:\s|{CSS_COMMENT_PATTERN})*"
    r"(?:\"((?:\\.|[^\"\\])*)\"|'((?:\\.|[^'\\])*)')",
)
CSS_URL_CAPTURE_GROUPS: Final = 3


def _first_group(match: re.Match[str]) -> str:
    """Return the populated capture from one CSS reference match.

    Returns:
        The represented resource value.

    """
    return next(group for group in match.groups() if group is not None)


def _matched_reference(source: str, index: int) -> tuple[str, int] | None:
    """Match an actual reference token beginning at one normal-state offset.

    Returns:
        Its value and ending offset, or ``None`` when neither grammar matches.

    """
    for pattern in (CSS_IMPORT_PATTERN, CSS_URL_PATTERN):
        match = pattern.match(source, index)
        if match is None:
            continue
        value = _first_group(match)
        if (
            len(match.groups()) == CSS_URL_CAPTURE_GROUPS
            and match.group(CSS_URL_CAPTURE_GROUPS) is not None
        ):
            value = re.sub(CSS_COMMENT_PATTERN, "", value)
        return value, match.end()
    return None


def _after_comment(source: str, index: int) -> int:
    """Return the offset after a CSS comment, including an unterminated one.

    Returns:
        The first offset outside the comment.

    """
    _body, terminator, remainder = source[index + 2 :].partition("*/")
    return len(source) - len(remainder) if terminator else len(source)


def _after_string(source: str, index: int) -> int:
    """Return the offset after one CSS string with escape-aware quote handling.

    Returns:
        The first offset outside the string, or the source length at EOF.

    """
    quote = source[index]
    characters = enumerate(source[index + 1 :], start=index + 1)
    for offset, character in characters:
        if character == quote:
            return offset + 1
        if character == "\\":
            next(characters, None)
    return len(source)


def _css_reference_values(source: str) -> tuple[str, ...]:
    """Extract resource values while skipping CSS comments and ordinary strings.

    Returns:
        Values from real ``url()`` functions and quoted ``@import`` rules.

    """
    values: list[str] = []
    index = 0
    for _iteration in range(len(source)):
        if index >= len(source):
            break
        if source.startswith("/*", index):
            index = _after_comment(source, index)
            continue
        if source[index] in "\"'":
            index = _after_string(source, index)
            continue
        matched = _matched_reference(source, index)
        if matched is None:
            index += 1
            continue
        value, index = matched
        values.append(value)
    return tuple(values)

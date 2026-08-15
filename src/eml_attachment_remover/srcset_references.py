"""Parse renderable URL candidates from HTML source-set attributes."""

from __future__ import annotations

import math
import re
from typing import Final, Literal

ASCII_WHITESPACE: Final = " \t\r\n\f"
NON_NEGATIVE_INTEGER_PATTERN: Final = re.compile(r"[0-9]+")
FLOAT_PATTERN: Final = re.compile(
    r"-?(?:(?:[0-9]+(?:\.[0-9]+)?)|(?:\.[0-9]+))"
    r"(?:[eE][+-]?[0-9]+)?",
)
type DescriptorKind = Literal["density", "height", "width"]
DENSITY_KIND: Final[DescriptorKind] = "density"
HEIGHT_KIND: Final[DescriptorKind] = "height"
WIDTH_KIND: Final[DescriptorKind] = "width"
PARENTHESIS_CLOSED: Final = object()
PARENTHESIS_OPEN: Final = object()


def _finish_descriptor(descriptors: list[str], current: list[str]) -> None:
    """Append and clear one non-empty descriptor under construction."""
    if current:
        descriptors.append("".join(current))
        current.clear()


def _consume_descriptors(value: str, index: int) -> tuple[tuple[str, ...], int]:
    """Consume descriptors through the next top-level candidate comma.

    Returns:
        Descriptor tokens and the position at which the next candidate begins.

    """
    descriptors: list[str] = []
    current: list[str] = []
    parenthesis_state = PARENTHESIS_CLOSED
    for character in value[index:]:
        index += 1
        if character == ")" and parenthesis_state is PARENTHESIS_OPEN:
            current.append(character)
            parenthesis_state = PARENTHESIS_CLOSED
        elif character == "(" and parenthesis_state is PARENTHESIS_CLOSED:
            current.append(character)
            parenthesis_state = PARENTHESIS_OPEN
        elif character == "," and parenthesis_state is PARENTHESIS_CLOSED:
            _finish_descriptor(descriptors, current)
            return tuple(descriptors), index
        elif character in ASCII_WHITESPACE and parenthesis_state is PARENTHESIS_CLOSED:
            _finish_descriptor(descriptors, current)
        else:
            current.append(character)
    _finish_descriptor(descriptors, current)
    return tuple(descriptors), index


def _consume_candidate(
    value: str,
    index: int,
) -> tuple[str, tuple[str, ...], int]:
    """Consume one URL and its descriptor tokens.

    Returns:
        The URL, descriptors, and position of the next candidate.

    """
    for character in value[index:]:
        if character not in f"{ASCII_WHITESPACE},":
            break
        index += 1
    start = index
    for character in value[index:]:
        if character in ASCII_WHITESPACE:
            break
        index += 1
    candidate = value[start:index]
    if not candidate or candidate.endswith(","):
        return candidate.removesuffix(","), (), index
    descriptors, index = _consume_descriptors(value, index)
    return candidate, descriptors, index


def _descriptor_kind(descriptor: str) -> DescriptorKind | None:
    """Return the valid descriptor kind, or ``None`` for invalid syntax.

    Returns:
        The parsed kind when syntax and numeric constraints are valid.

    """
    number, suffix = descriptor[:-1], descriptor[-1:]
    integer = NON_NEGATIVE_INTEGER_PATTERN.fullmatch(number)
    if suffix in {"w", "h"}:
        if integer is None or not any(digit != "0" for digit in number):
            return None
        return WIDTH_KIND if suffix == "w" else HEIGHT_KIND
    if suffix != "x" or FLOAT_PATTERN.fullmatch(number) is None:
        return None
    density = float(number)
    return DENSITY_KIND if math.isfinite(density) and density >= 0 else None


def _valid_descriptors(descriptors: tuple[str, ...]) -> bool:
    """Return whether descriptor syntax can produce an image candidate.

    Returns:
        ``True`` exactly when the HTML source-set parser accepts the candidate.

    """
    kinds: set[DescriptorKind] = set()
    for descriptor in descriptors:
        kind = _descriptor_kind(descriptor)
        if kind is None:
            return False
        if kind in kinds:
            return False
        allowed = (
            (kind == WIDTH_KIND and DENSITY_KIND not in kinds)
            or (kind == DENSITY_KIND and not kinds)
            or (kind == HEIGHT_KIND and DENSITY_KIND not in kinds)
        )
        if not allowed:
            return False
        kinds.add(kind)
    return HEIGHT_KIND not in kinds or WIDTH_KIND in kinds


def _srcset_urls(value: str) -> tuple[str, ...]:
    """Extract URLs accepted by the HTML source-set parsing algorithm.

    Returns:
        Valid candidate URLs, including intact data URLs containing commas.

    """
    candidates: list[str] = []
    index = 0
    for _candidate_budget in value:
        candidate, descriptors, index = _consume_candidate(value, index)
        if candidate and _valid_descriptors(descriptors):
            candidates.append(candidate)
    return tuple(candidates)

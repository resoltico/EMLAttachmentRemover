"""Extract release prose from the canonical changelog without rewriting it."""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import date
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
HEADING = re.compile(r"## \[([^\]]+)\](?: - ([0-9]{4}-[0-9]{2}-[0-9]{2}))?")
FENCE = re.compile(r" {0,3}(`{3,}|~{3,})(.*)")
REFERENCE = re.compile(r" {0,3}\[([^\]]+)\]:[ \t]+\S.*")


class ReleaseError(RuntimeError):
    """Reject an ambiguous or inconsistent release operation."""


def require(message: str, *, condition: bool) -> None:
    """Raise a release error unless one invariant holds.

    Raises:
        ReleaseError: If ``condition`` is false.

    """
    if not condition:
        raise ReleaseError(message)


@dataclass(frozen=True, slots=True)
class _Section:
    """Identify one validated changelog section."""

    version: str
    start: int


def _outside_fences(lines: list[str]) -> Iterator[tuple[int, str]]:
    """Yield structural lines outside fenced examples.

    Yields:
        Zero-based offsets and ordinary Markdown lines.

    """
    active = ""
    for index, line in enumerate(lines):
        match = FENCE.fullmatch(line)
        if active:
            if (
                match
                and not match[2].strip()
                and match[1][0] == active[0]
                and len(match[1]) >= len(active)
            ):
                active = ""
        elif match:
            require(
                "Invalid backtick fence in CHANGELOG.md",
                condition=match[1][0] != "`" or "`" not in match[2],
            )
            active = match[1]
        else:
            yield index, line
    require("Unclosed fence in CHANGELOG.md", condition=not active)


def _section(line: str, index: int) -> _Section:
    """Parse one supported level-two changelog heading.

    Returns:
        One validated version heading and its location.

    Raises:
        ReleaseError: If the heading is not an Unreleased or dated stable version.

    """
    match = HEADING.fullmatch(line)
    if match is None:
        message = "Invalid second-level changelog heading"
        raise ReleaseError(message)
    version, released = match.groups()
    if version == "Unreleased":
        require("Unreleased must not have a date", condition=released is None)
    else:
        require(
            "Invalid stable version", condition=VERSION.fullmatch(version) is not None
        )
        require("Release heading must have an ISO date", condition=released is not None)
        try:
            date.fromisoformat(released or "")
        except ValueError as error:
            message = "Invalid release date in CHANGELOG.md"
            raise ReleaseError(message) from error
    return _Section(version, index)


def _structure(lines: list[str]) -> tuple[list[_Section], int]:
    """Validate sections and find the optional common reference footer.

    Returns:
        Ordered headings and the footer start offset.

    """
    sections: list[_Section] = []
    labels: set[str] = set()
    footer = len(lines)
    for index, line in _outside_fences(lines):
        reference = REFERENCE.fullmatch(line)
        if reference:
            footer = min(footer, index)
            label = " ".join(reference[1].split()).casefold()
            require(
                "Duplicate changelog reference definition",
                condition=label not in labels,
            )
            labels.add(label)
        elif footer != len(lines):
            require(
                "Reference definitions must form one footer", condition=not line.strip()
            )
        elif re.match(r" {0,3}##(?:[ \t]|$)", line):
            sections.append(_section(line, index))
    footer_is_valid = all(
        not line.strip() or REFERENCE.fullmatch(line) for line in lines[footer:]
    )
    require(
        "Reference footer contains non-definition content", condition=footer_is_valid
    )
    return sections, footer


def extract_release(changelog: str, version: str) -> str:
    """Return selected release content, excluding its validated dated heading.

    Returns:
        Canonical Markdown with one terminal newline.

    """
    require(
        "Expected a stable version", condition=VERSION.fullmatch(version) is not None
    )
    text = changelog.replace("\r\n", "\n")
    require(
        "Invalid changelog encoding", condition="\r" not in text and "\x00" not in text
    )
    lines = text.split("\n")
    sections, footer = _structure(lines)
    versions = [section.version for section in sections]
    require(
        "Duplicate changelog section", condition=len(versions) == len(set(versions))
    )
    require(
        "Current release must follow Unreleased",
        condition=versions[:2] == ["Unreleased", version],
    )
    start = sections[1].start
    third_section = 2
    end = sections[third_section].start if len(sections) > third_section else footer
    require(
        "Release entry must contain change text",
        condition=any(
            line.strip() and not line.lstrip().startswith("#")
            for line in lines[start + 1 : end]
        ),
    )
    body = "\n".join(lines[start + 1 : end]).strip("\n")
    return body + "\n"

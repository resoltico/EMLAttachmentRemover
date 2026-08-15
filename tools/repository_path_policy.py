"""Define cross-platform path portability rules for the public source tree."""

from __future__ import annotations

import unicodedata
from dataclasses import dataclass
from pathlib import PurePosixPath
from typing import Final

WINDOWS_FORBIDDEN_CHARACTERS: Final = frozenset('<>:"\\|?*')
WINDOWS_RESERVED_STEMS: Final = frozenset({
    "aux",
    "con",
    "conin$",
    "conout$",
    "nul",
    "prn",
    *(f"com{number}" for number in range(1, 10)),
    *(f"lpt{number}" for number in range(1, 10)),
})
ASCII_CONTROL_LIMIT: Final = 32


@dataclass(frozen=True, slots=True)
class PathIssue:
    """Describe one cross-platform hazard for a relative public path."""

    relative_path: str
    message: str


def _component_messages(component: str) -> tuple[str, ...]:
    """Return portability diagnostics for one path component.

    Returns:
        Every Windows, Unicode, or control-character issue.

    """
    try:
        component.encode()
    except UnicodeEncodeError:
        return ("path component is not valid Unicode text",)
    messages: list[str] = []
    if any(ord(character) < ASCII_CONTROL_LIMIT for character in component):
        messages.append("path component contains a control character")
    if WINDOWS_FORBIDDEN_CHARACTERS & set(component):
        messages.append("path component contains a Windows-forbidden character")
    if component.endswith((" ", ".")):
        messages.append("path component ends with a Windows-ignored space or dot")
    stem = component.partition(".")[0].casefold().rstrip(" .")
    if stem in WINDOWS_RESERVED_STEMS:
        messages.append("path component uses a Windows reserved device name")
    return tuple(messages)


def _component_key(component: str) -> str:
    """Return one cross-platform component collision key.

    Returns:
        A case-folded NFC name with Windows-ignored suffixes removed.

    """
    return unicodedata.normalize("NFC", component).casefold().rstrip(" .")


def audit_paths(relative_paths: tuple[str, ...]) -> tuple[PathIssue, ...]:
    """Return component and prefix-collision issues for public paths.

    Returns:
        Deterministic diagnostics in input and component order.

    """
    issues: list[PathIssue] = []
    seen_prefixes: dict[tuple[str, ...], str] = {}
    for relative_path in relative_paths:
        components = PurePosixPath(relative_path).parts
        for component in components:
            issues.extend(
                PathIssue(relative_path, message)
                for message in _component_messages(component)
            )
        prefix_parts: tuple[str, ...] = ()
        for component in components:
            prefix_parts = (*prefix_parts, component)
            prefix = PurePosixPath(*prefix_parts).as_posix()
            key = tuple(_component_key(item) for item in prefix_parts)
            previous = seen_prefixes.setdefault(key, prefix)
            if previous != prefix:
                issues.append(
                    PathIssue(
                        relative_path,
                        f"public path collides cross-platform with {previous!r}",
                    ),
                )
                break
    return tuple(issues)

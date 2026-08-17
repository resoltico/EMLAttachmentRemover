"""Define immutable public audit records for text-only MIME transformation."""

from __future__ import annotations

from dataclasses import dataclass

type MimePath = tuple[int, ...]


@dataclass(frozen=True, slots=True)
class SelectedPlainTextBody:
    """Identify the one plain-text body selected for the derived message."""

    path: MimePath
    content_type: str


@dataclass(frozen=True, slots=True)
class DiscardedBodyRepresentation:
    """Identify an unselected MIME body representation discarded as a unit."""

    path: MimePath
    content_type: str


@dataclass(frozen=True, slots=True)
class DiscardedBodyResource:
    """Describe one resource belonging to discarded body representations."""

    path: MimePath
    referenced_by: tuple[MimePath, ...]
    content_type: str
    filename: str | None
    disposition: str | None

"""Bounded source skeletons for independent CPython MIME structure checks."""

from __future__ import annotations

from typing import TYPE_CHECKING

from .domain import AppError, ExitCode
from .mime_identifiers import parse_message_identifier

if TYPE_CHECKING:
    from .mime_raw import RawNode

_OPAQUE_PLACEHOLDER = b"X"
_ATTACHMENT_DISPOSITION = "attachment"


def build_skeleton(raw: bytes, root: RawNode) -> bytes:
    """Replace only independently authorized opaque payload spans.

    Returns:
        The original bytes when no source child is opaque, otherwise a bounded
        temporary skeleton suitable only for the stdlib structural check.

    Raises:
        AppError: If raw-index opaque ownership or a payload span is unsound.

    """
    edits = _opaque_payload_edits(root)
    if not edits:
        return raw
    position = 0
    chunks: list[bytes] = []
    for start, end in edits:
        if start < position or end < start or end > len(raw):
            raise AppError(ExitCode.PARSE_ERROR, "invalid opaque skeleton span")
        chunks.extend((raw[position:start], _OPAQUE_PLACEHOLDER))
        position = end
    chunks.append(raw[position:])
    return b"".join(chunks)


def _opaque_payload_edits(root: RawNode) -> list[tuple[int, int]]:
    """Derive and validate every direct opaque source child without planner facts.

    Returns:
        Sorted nonoverlapping source payload spans that may be skeletonized.

    Raises:
        AppError: If an opaque marker disagrees with independently derived context.

    """
    edits: list[tuple[int, int]] = []
    pending = [root]
    while pending:
        parent = pending.pop()
        opaque_index = (
            _related_root_index(parent)
            if parent.media_type == "multipart/related"
            else -1
        )
        for index, child in enumerate(parent.children):
            expected = (
                index != opaque_index
                if parent.media_type == "multipart/related"
                else child.disposition is not None
                and child.disposition.token == _ATTACHMENT_DISPOSITION
            )
            if child.opaque != expected:
                raise AppError(
                    ExitCode.PARSE_ERROR, "opaque raw-index ownership mismatch"
                )
            if expected:
                edits.append(_payload_edit(child))
            else:
                pending.append(child)
    return sorted(edits)


def _related_root_index(parent: RawNode) -> int:
    """Select one direct retained related root from source headers alone.

    Returns:
        The direct child index selected by a related ``start`` control or zero.

    Raises:
        AppError: If a declared start identifier is not one unique direct child.

    """
    start = parent.parameter(b"start")
    if start is None:
        return 0
    identifier = parse_message_identifier(start, field="multipart/related start")
    matches = [
        index
        for index, child in enumerate(parent.children)
        if _content_identifier(child) == identifier
    ]
    if len(matches) != 1:
        raise AppError(ExitCode.PARSE_ERROR, "related start has no unique direct root")
    return matches[0]


def _content_identifier(node: RawNode) -> bytes | None:
    """Return one source Content-ID without relying on raw parser role flags.

    Returns:
        The exact parsed identifier, or ``None`` if source has no Content-ID.

    """
    values = node.header_map.get(b"content-id")
    return (
        None
        if not values
        else parse_message_identifier(values[0].value, field="Content-ID")
    )


def _payload_edit(node: RawNode) -> tuple[int, int]:
    """Return one opaque payload-only span that cannot touch outer syntax.

    Returns:
        The exact source payload span excluding its outer headers and delimiter.

    Raises:
        AppError: If the raw node lacks a confined payload boundary.

    """
    end = node.payload_end
    if end is None or node.body_start > end or end > node.end:
        raise AppError(ExitCode.PARSE_ERROR, "opaque payload has no confined span")
    return node.body_start, end

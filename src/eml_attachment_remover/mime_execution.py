"""Raw-span candidate construction for a frozen MIME policy plan."""

from __future__ import annotations

import codecs
import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING

from .domain import AppError, ExitCode, Removal
from .mime_removals import RemovalIndex

if TYPE_CHECKING:
    from .mime_raw import RawMimeTree

TRANSPORT_HEADERS = frozenset({
    b"dkim-signature",
    b"domainkey-signature",
    b"arc-seal",
    b"arc-message-signature",
    b"arc-authentication-results",
})
CHANGED_HEADERS = frozenset({b"content-md5", b"content-length", b"lines"})


@dataclass(frozen=True, slots=True)
class Candidate:
    """Candidate bytes and the exact physical field names removed from source."""

    raw: bytes
    digest: str
    stripped_headers: tuple[str, ...]


def _header_edits(
    tree: RawMimeTree, index: RemovalIndex
) -> tuple[list[tuple[int, int]], list[str]]:
    """Return stale-header spans for changed retained entities and the outer root.

    Returns:
        Physical source spans to delete and their normalized names for provenance.

    """
    changed = index.changed_ancestor_paths(tree.root)
    if not changed:
        return [], []
    edits: list[tuple[int, int]] = []
    names: list[str] = []
    for path in changed:
        node = tree.by_path[path]
        allowed = set(CHANGED_HEADERS)
        if path == ():
            allowed.update(TRANSPORT_HEADERS)
            allowed.add(b"x-ms-has-attach")
        for header in node.headers:
            if header.name in allowed:
                edits.append((header.start, header.end))
                names.append(codecs.ascii_decode(header.name)[0])
    return edits, names


def _nonoverlapping(edits: list[tuple[int, int]]) -> list[tuple[int, int]]:
    """Sort source spans and reject any overlapping or empty edit.

    Returns:
        Ascending source spans suitable for byte-preserving deletion.

    Raises:
        AppError: If plans contain an empty or overlapping raw edit.

    """
    ordered = sorted(edits)
    previous_end: int | None = None
    for start, end in ordered:
        if (
            start < 0
            or start >= end
            or (previous_end is not None and start < previous_end)
        ):
            raise AppError(ExitCode.VERIFICATION_ERROR, "overlapping raw MIME edits")
        previous_end = end
    return ordered


def _apply(raw: bytes, edits: list[tuple[int, int]]) -> bytes:
    """Delete only the indexed spans, retaining every byte between them unchanged.

    Returns:
        Candidate bytes built exclusively by source-span deletion.

    """
    if not edits:
        return raw
    iterator = iter(edits)
    first_start, position = next(iterator)
    pieces = [raw[:first_start]]
    for start, end in iterator:
        pieces.append(raw[position:start])
        position = end
    pieces.append(raw[position:])
    return b"".join(pieces)


def build_candidate(tree: RawMimeTree, removals: tuple[Removal, ...]) -> Candidate:
    """Execute the frozen policy through source-byte deletion only.

    Returns:
        Candidate bytes, digest, and changed-header provenance.

    Raises:
        AppError: If a planned removal is unindexed or touches the logical root.

    """
    paths = {removal.path for removal in removals}
    index = RemovalIndex.from_roots(paths)
    edits: list[tuple[int, int]] = []
    for path in paths:
        node = tree.by_path.get(path)
        if node is None:
            raise AppError(ExitCode.VERIFICATION_ERROR, "removal root is unindexed")
        if node.delete_start is None or node.delete_end is None:
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "policy attempted to remove root MIME node"
            )
        edits.append((node.delete_start, node.delete_end))
    header_edits, stripped = _header_edits(tree, index)
    edits.extend(header_edits)
    raw = _apply(tree.raw, _nonoverlapping(edits))
    return Candidate(raw, hashlib.sha256(raw).hexdigest(), tuple(sorted(set(stripped))))

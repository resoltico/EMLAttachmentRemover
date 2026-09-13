"""Independent CPython checks for raw-indexed MIME trees."""

from __future__ import annotations

import codecs
from dataclasses import dataclass
from email import errors, policy
from email.message import EmailMessage
from email.parser import BytesParser
from typing import TYPE_CHECKING

from .domain import AppError, ExitCode, MimePath

if TYPE_CHECKING:
    from .mime_raw import RawNode


@dataclass(frozen=True, slots=True)
class StdlibValidationWork:
    """Bounded CPython-tree traversal receipt."""

    node_visits: int
    child_edge_visits: int


def parse_stdlib(raw: bytes) -> EmailMessage:
    """Return CPython's separately parsed MIME tree.

    Returns:
        CPython's independent parsed message tree.

    Raises:
        AppError: If CPython rejects the source structure.

    """
    try:
        policy_value = policy.default.clone(refold_source="none")
        return BytesParser(policy=policy_value).parsebytes(raw)
    except (errors.MessageParseError, RecursionError, ValueError) as exc:
        raise AppError(
            ExitCode.PARSE_ERROR, "stdlib MIME parser rejected source"
        ) from exc


def validate_stdlib_tree(
    message: EmailMessage, raw_nodes: dict[MimePath, RawNode]
) -> StdlibValidationWork:
    """Compare CPython structure with raw ownership outside opaque descendants.

    Returns:
        Exact node and child-edge work performed during the comparison.

    Raises:
        AppError: If CPython and raw ownership disagree outside opaque roots.

    """
    pending: list[tuple[MimePath, EmailMessage]] = [((), message)]
    visited: set[MimePath] = set()
    edges = 0
    while pending:
        path, part = pending.pop()
        node = raw_nodes.get(path)
        if node is None:
            raise AppError(
                ExitCode.PARSE_ERROR, "stdlib MIME tree has an unindexed part"
            )
        visited.add(path)
        _validate_defects(part, node)
        _validate_headers(part)
        _compare_ownership(part, node)
        if node.opaque:
            continue
        children = _children(part)
        edges += len(children)
        if len(children) != len(node.children):
            raise AppError(
                ExitCode.PARSE_ERROR, "raw MIME child count disagrees with stdlib"
            )
        pending.extend(((*path, index), child) for index, child in enumerate(children))
    if visited != set(raw_nodes):
        raise AppError(ExitCode.PARSE_ERROR, "raw MIME index has unverified nodes")
    return StdlibValidationWork(len(visited), edges)


def _children(part: EmailMessage) -> list[EmailMessage]:
    """Return checked multipart children.

    Returns:
        Parsed child messages, or an empty list for a leaf.

    Raises:
        AppError: If CPython exposes a non-message child.

    """
    payload = part.get_payload()
    if not isinstance(payload, list):
        return []
    children = [child for child in payload if isinstance(child, EmailMessage)]
    if len(children) != len(payload):
        raise AppError(ExitCode.PARSE_ERROR, "stdlib parser has non-message child")
    return children


def _validate_defects(part: EmailMessage, node: RawNode) -> None:
    """Reject CPython defects outside an opaque removal root.

    Raises:
        AppError: If CPython reports an unconfined structural defect.

    """
    if node.opaque:
        return
    headerless = not node.headers
    defects = tuple(
        defect
        for defect in part.defects
        if not (
            headerless and isinstance(defect, errors.MissingHeaderBodySeparatorDefect)
        )
    )
    if defects:
        raise AppError(ExitCode.PARSE_ERROR, "stdlib MIME parser reported a defect")


def _validate_headers(part: EmailMessage) -> None:
    """Reject CPython physical structured-header defects.

    Raises:
        AppError: If a physical header has CPython parser defects.

    """
    for name in part:
        if part[name].defects:
            raise AppError(ExitCode.PARSE_ERROR, "MIME header parser reported a defect")


def _compare_ownership(part: EmailMessage, node: RawNode) -> None:
    """Require raw and CPython header ownership and declared type to agree.

    Raises:
        AppError: If physical header ownership or declared type differs.

    """
    raw_names = tuple(header.name for header in node.headers)
    stdlib_names = tuple(
        codecs.ascii_encode(name.lower())[0] for name, _ in part.raw_items()
    )
    if raw_names != stdlib_names:
        raise AppError(
            ExitCode.PARSE_ERROR, "raw MIME header ownership disagrees with stdlib"
        )
    if node.media_type != part.get_content_type().lower():
        raise AppError(
            ExitCode.PARSE_ERROR, "raw MIME media type disagrees with stdlib"
        )

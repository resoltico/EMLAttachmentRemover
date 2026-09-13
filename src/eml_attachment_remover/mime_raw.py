"""Raw MIME tree indexing with byte spans and stdlib structural cross-checking."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Final

from .domain import AppError, ExitCode, MimePath
from .mime_headers import Header, is_header_name, line_end, parse_headers
from .mime_identifiers import parse_message_identifier
from .mime_stdlib_check import StdlibValidationWork, parse_stdlib, validate_stdlib_tree
from .mime_validation import ContentSpec, content_specs

MAX_NODES: Final = 20_000
MAX_DEPTH: Final = 64
MAX_CHILDREN: Final = 10_000
MAX_TOTAL_HEADERS: Final = 8 * 1024 * 1024
ATTACHMENT_DISPOSITION: Final = "attachment"


@dataclass(slots=True)
class RawNode:
    """A source-indexed MIME entity."""

    path: MimePath
    start: int
    end: int
    body_start: int
    headers: tuple[Header, ...]
    content_type: ContentSpec
    disposition: ContentSpec | None
    cte: str
    children: list[RawNode] = field(default_factory=list)
    opaque: bool = False
    delete_start: int | None = None
    delete_end: int | None = None
    payload_end: int | None = None

    @property
    def media_type(self) -> str:
        """Normalized type/subtype."""
        return self.content_type.token

    @property
    def is_multipart(self) -> bool:
        """Whether this node declares a multipart container."""
        return self.media_type.startswith("multipart/")

    @property
    def header_map(self) -> dict[bytes, list[Header]]:
        """Physical fields grouped by lower-case name."""
        result: dict[bytes, list[Header]] = {}
        for header in self.headers:
            result.setdefault(header.name, []).append(header)
        return result

    def parameter(self, name: bytes) -> bytes | None:
        """Return one already validated raw parameter value.

        Returns:
            The exact normalized parameter value, if the field declared it.

        """
        return self.content_type.parameters.get(name)


@dataclass(frozen=True, slots=True)
class RawMimeTree:
    """The root node and source data for a validated message."""

    raw: bytes
    root: RawNode
    nodes: tuple[RawNode, ...]
    by_path: dict[MimePath, RawNode]
    stdlib_work: StdlibValidationWork


def _find_separator(raw: bytes, start: int, end: int) -> tuple[int, int]:
    """Find the exact header/body separator in one entity.

    Returns:
        The separator start offset and its byte length.

    Raises:
        AppError: If the entity has no physical separator.

    """
    matches = [
        (raw.find(marker, start, end), len(marker))
        for marker in (b"\r\n\r\n", b"\n\n", b"\r\r")
    ]
    positions = [(position, length) for position, length in matches if position >= 0]
    if not positions:
        raise AppError(ExitCode.PARSE_ERROR, "MIME entity has no header/body separator")
    return min(positions)


def _delimiter_lines(
    raw: bytes, start: int, end: int, boundary: bytes
) -> list[tuple[int, int, bool]]:
    """Find only whole-line multipart delimiters in the containing body range.

    Returns:
        Source spans and closing markers for recognized delimiter lines.

    Raises:
        AppError: If a delimiter cursor cannot reach the containing body end.

    """
    prefix = b"--" + boundary
    result: list[tuple[int, int, bool]] = []
    position = start
    for _line in range(end - start):
        if position >= end:
            break
        end_of_line = _advanced_delimiter_cursor(position, line_end(raw, position, end))
        line = raw[position:end_of_line].rstrip(b"\r\n")
        if line.startswith(prefix):
            tail = line[len(prefix) :]
            closing = tail.startswith(b"--")
            if closing:
                tail = tail[2:]
            if not tail.strip(b" \t"):
                result.append((position, end_of_line, closing))
        position = end_of_line
    if position < end:
        raise AppError(ExitCode.PARSE_ERROR, "MIME delimiter cursor did not advance")
    return result


def _advanced_delimiter_cursor(position: int, next_position: int) -> int:
    """Return a strictly advanced multipart delimiter cursor or fail closed.

    Returns:
        The validated next delimiter offset.

    Raises:
        AppError: If the proposed offset does not strictly advance.

    """
    if next_position <= position:
        raise AppError(ExitCode.PARSE_ERROR, "MIME delimiter cursor did not advance")
    return next_position


def _payload_end(raw: bytes, boundary_start: int) -> int:
    """Exclude the transport line break immediately introducing a MIME delimiter.

    Returns:
        The exclusive payload offset before the delimiter's preceding line ending.

    """
    if raw[boundary_start - 2 : boundary_start] == b"\r\n":
        return boundary_start - 2
    if raw[boundary_start - 1 : boundary_start] in {b"\n", b"\r"}:
        return boundary_start - 1
    return boundary_start


def _entity_headers(
    raw: bytes, start: int, end: int
) -> tuple[tuple[Header, ...], int, int]:
    """Return physical headers and exact body start, accepting a headerless entity.

    Returns:
        Parsed fields, body start offset, and physical header byte count.

    Raises:
        AppError: If a header-like entity lacks a valid physical separator.

    """
    if not _first_line_is_header_like(raw, start, end):
        return (), start, 0
    try:
        separator, separator_length = _find_separator(raw, start, end)
    except AppError as exc:
        raise AppError(
            ExitCode.PARSE_ERROR, "header-like MIME entity lacks a body separator"
        ) from exc
    headers = parse_headers(raw, start, separator)
    return headers, separator + separator_length, separator - start


def _first_line_is_header_like(raw: bytes, start: int, end: int) -> bool:
    """Return whether an entity begins with a colon-bearing physical header line.

    Returns:
        Whether its initial physical line can be interpreted as a header field.

    """
    first_line = raw[start : line_end(raw, start, end)]
    name, colon, _value = first_line.partition(b":")
    return bool(colon) and is_header_name(name)


def _parse_node(
    raw: bytes, start: int, end: int, path: MimePath, totals: list[int]
) -> RawNode:
    """Build one raw tree node recursively within a bounded entity span.

    Returns:
        The source-indexed entity, including recursively indexed children.

    """
    node = _shallow_node(raw, start, end, path, totals)
    _parse_descendants(raw, node, totals)
    return node


def _shallow_node(
    raw: bytes, start: int, end: int, path: MimePath, totals: list[int]
) -> RawNode:
    """Index one entity's headers without descending into a possible container.

    Returns:
        A source-indexed entity with no parsed children yet.

    Raises:
        AppError: If limits or the entity's outer header controls are invalid.

    """
    _count_node(path, totals)
    headers, body_start, header_bytes = _entity_headers(raw, start, end)
    totals[1] += header_bytes
    if totals[1] > MAX_TOTAL_HEADERS:
        raise AppError(
            ExitCode.PARSE_ERROR, "MIME message exceeds cumulative header limit"
        )
    content_type, disposition, cte = content_specs(headers)
    return RawNode(
        path, start, end, body_start, headers, content_type, disposition, cte
    )


def _parse_descendants(raw: bytes, node: RawNode, totals: list[int]) -> None:
    """Index descendants unless the contextual parent already proves opaque removal."""
    if not node.is_multipart:
        return
    _parse_multipart(raw, node, totals)


def _count_node(path: MimePath, totals: list[int]) -> None:
    """Enforce bounded MIME depth and total node count.

    Raises:
        AppError: If the source exceeds a public node or depth budget.

    """
    if len(path) > MAX_DEPTH:
        raise AppError(ExitCode.PARSE_ERROR, "MIME tree exceeds depth limit", path)
    totals[0] += 1
    if totals[0] > MAX_NODES:
        raise AppError(ExitCode.PARSE_ERROR, "MIME tree exceeds node limit", path)


def _parse_multipart(raw: bytes, node: RawNode, totals: list[int]) -> None:
    """Index every direct child of one validated multipart entity.

    Raises:
        AppError: If its boundary or direct-child structure is invalid.

    """
    boundary = node.parameter(b"boundary")
    if not boundary:
        raise AppError(
            ExitCode.PARSE_ERROR, "multipart entity lacks a boundary", node.path
        )
    delimiters = _delimiter_lines(raw, node.body_start, node.end, boundary)
    normal = _opening_delimiters(delimiters, node.path)
    if len(normal) > MAX_CHILDREN:
        raise AppError(
            ExitCode.PARSE_ERROR, "multipart exceeds direct-child limit", node.path
        )
    children = _shallow_children(raw, node, normal, delimiters, totals)
    related_root = _related_root_index(node, children)
    for index, child in enumerate(children):
        child.opaque = _opaque_child(node, child, index, related_root)
        if not child.opaque:
            _parse_descendants(raw, child, totals)
    node.children.extend(children)


def _shallow_children(
    raw: bytes,
    parent: RawNode,
    normal: list[tuple[int, int, bool]],
    delimiters: list[tuple[int, int, bool]],
    totals: list[int],
) -> list[RawNode]:
    """Index every direct child outer entity before descending into any child.

    Returns:
        Direct child nodes with deletion and payload boundary spans.

    """
    children: list[RawNode] = []
    for index, (start, finish, _closing) in enumerate(normal):
        next_start = delimiters[index + 1][0]
        child = _shallow_node(raw, finish, next_start, (*parent.path, index), totals)
        child.delete_start = start
        child.delete_end = next_start
        child.payload_end = _payload_end(raw, next_start)
        children.append(child)
    return children


def _related_root_index(parent: RawNode, children: list[RawNode]) -> int | None:
    """Return a related compound's direct root index before opaque child descent.

    Returns:
        The selected direct root index, or ``None`` outside a related compound.

    Raises:
        AppError: If a declared related start value cannot select one direct root.

    """
    if parent.media_type != "multipart/related":
        return None
    start = parent.parameter(b"start")
    if start is None:
        return 0
    identifier = parse_message_identifier(start, field="multipart/related start")
    matches = [
        index
        for index, child in enumerate(children)
        if _content_id(child) == identifier
    ]
    if len(matches) != 1:
        raise AppError(
            ExitCode.PARSE_ERROR, "related start has no unique direct root", parent.path
        )
    return matches[0]


def _content_id(node: RawNode) -> bytes | None:
    """Return a child Content-ID that was already structurally validated.

    Returns:
        Exact Content-ID octets, or ``None`` if the child has no such field.

    """
    for header in node.headers:
        if header.name == b"content-id":
            return parse_message_identifier(header.value, field="Content-ID")
    return None


def _opaque_child(
    parent: RawNode,
    child: RawNode,
    index: int,
    related_root: int | None,
) -> bool:
    """Return whether container context proves a direct child is a removal root.

    Returns:
        Whether the child is structurally removable without parsing descendants.

    """
    if parent.media_type == "multipart/related":
        return index != related_root
    disposition = child.disposition
    return disposition is not None and disposition.token == ATTACHMENT_DISPOSITION


def _opening_delimiters(
    delimiters: list[tuple[int, int, bool]], path: MimePath
) -> list[tuple[int, int, bool]]:
    """Validate multipart delimiter ordering and return its opening delimiters.

    Returns:
        The nonclosing delimiter records that introduce child entities.

    Raises:
        AppError: If the multipart does not have a usable closing boundary.

    """
    if not delimiters or not delimiters[-1][2]:
        raise AppError(
            ExitCode.PARSE_ERROR, "multipart entity lacks a closing boundary", path
        )
    if any(closing for _start, _end, closing in delimiters[:-1]):
        raise AppError(
            ExitCode.PARSE_ERROR,
            "multipart content follows a closing boundary",
            path,
        )
    normal = delimiters[:-1]
    if not normal:
        raise AppError(
            ExitCode.PARSE_ERROR, "multipart entity has no child parts", path
        )
    return normal


def parse_raw_mime(raw: bytes) -> RawMimeTree:
    """Validate source structure and return its non-overlapping raw index.

    Returns:
        The raw source, root, source-order nodes, and path lookup index.

    """
    root = _parse_node(raw, 0, len(raw), (), [0, 0])
    nodes = iter_nodes(root)
    by_path = {node.path: node for node in nodes}
    stdlib = parse_stdlib(raw)
    stdlib_work = validate_stdlib_tree(stdlib, by_path)
    return RawMimeTree(raw, root, nodes, by_path, stdlib_work)


def iter_nodes(root: RawNode) -> tuple[RawNode, ...]:
    """Return every node in stable depth-first source order.

    Returns:
        All indexed entities in depth-first source order.

    """
    result: list[RawNode] = []
    pending = [root]
    while pending:
        node = pending.pop()
        result.append(node)
        pending.extend(reversed(node.children))
    return tuple(result)

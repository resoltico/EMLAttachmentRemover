"""The closed contextual policy for MIME-pruned working copies."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Final

from .domain import AppError, DecisionAction, ExitCode, Removal, RemovalReason
from .mime_identifiers import (
    first_non_cfws,
    parse_message_identifier,
    parse_message_identifier_sequence,
)

if TYPE_CHECKING:
    from .mime_raw import RawNode

TEXT_TYPES: Final = frozenset({"text/plain", "text/html"})
OPEN_ANGLE: Final = ord("<")
PROTECTED_TYPES: Final = frozenset({
    "multipart/signed",
    "multipart/encrypted",
    "application/pkcs7-mime",
    "application/x-pkcs7-mime",
    "application/pkcs7-signature",
    "application/x-pkcs7-signature",
    "application/pgp-encrypted",
    "application/pgp-signature",
})


@dataclass(frozen=True, slots=True)
class PolicyResult:
    """The action table and permitted removal roots for one tree."""

    actions: dict[tuple[int, ...], DecisionAction]
    removals: tuple[Removal, ...]


def _disposition(node: RawNode) -> str | None:
    """Return the normalized disposition token when present.

    Returns:
        The disposition token, or ``None`` if no disposition field was supplied.

    """
    return None if node.disposition is None else node.disposition.token


def _has_filename(node: RawNode) -> bool:
    """Return whether either filename parameter makes a role ambiguous.

    Returns:
        Whether name or filename metadata requires an explicit inline body context.

    """
    disposition = node.disposition
    return b"name" in node.content_type.parameters or (
        disposition is not None and b"filename" in disposition.parameters
    )


def _cid(node: RawNode) -> bytes | None:
    """Parse an exact bracketed Content-ID without normalization or URL decoding.

    Returns:
        Exact identifier octets, or ``None`` when the node has no Content-ID.

    """
    values = node.header_map.get(b"content-id", [])
    if not values:
        return None
    return parse_message_identifier(values[0].value, field="Content-ID")


def _parse_start(value: bytes) -> bytes:
    """Parse the one exact message identifier accepted by related start.

    Returns:
        Exact related-start identifier octets without RFC wrapper syntax.

    """
    return parse_message_identifier(value, field="multipart/related start")


def _related_ids(node: RawNode) -> dict[bytes, RawNode]:
    """Collect unique exact IDs from an entire related compound.

    Returns:
        Exact Content-ID octets mapped to their unique source nodes.

    Raises:
        AppError: If two related nodes have the same canonical Content-ID.

    """
    ids: dict[bytes, RawNode] = {}
    pending = list(node.children)
    while pending:
        current = pending.pop()
        identifier = _cid(current)
        if identifier is not None:
            if identifier in ids:
                raise AppError(
                    ExitCode.TRANSFORMATION_UNAVAILABLE,
                    "duplicate canonical Content-ID in multipart/related",
                    current.path,
                )
            ids[identifier] = current
        pending.extend(current.children)
    return ids


def _start_info_ids(value: bytes) -> tuple[bytes, ...] | None:
    """Return CID-form start-info identifiers, or ``None`` for a literal value.

    Returns:
        Exact CID-form entries, or ``None`` when start-info is a literal.

    Raises:
        AppError: If CID-form start-info repeats an identifier.

    """
    position = first_non_cfws(value)
    if position == len(value):
        return None
    if value[position] != OPEN_ANGLE:
        return None
    identifiers = parse_message_identifier_sequence(value, field="related start-info")
    if len(set(identifiers)) != len(identifiers):
        raise AppError(
            ExitCode.TRANSFORMATION_UNAVAILABLE, "duplicate related start-info ID"
        )
    return identifiers


class _Classifier:
    """Own one classification traversal and its explicit action map."""

    def __init__(self) -> None:
        """Initialize empty decisions."""
        self.actions: dict[tuple[int, ...], DecisionAction] = {}
        self.removals: list[Removal] = []

    def classify(self, root: RawNode) -> PolicyResult:
        """Classify a root and require at least one retained body leaf.

        Returns:
            Complete action table and permitted removal roots.

        Raises:
            AppError: If no supported retained body leaf remains.

        """
        self._body(root)
        if not self._has_retained_leaf(root):
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE, "no supported body remains"
            )
        return PolicyResult(self.actions, tuple(self.removals))

    def _has_retained_leaf(self, node: RawNode) -> bool:
        """Return whether a kept leaf exists outside a removal root.

        Returns:
            Whether the node subtree retains at least one supported body leaf.

        """
        action = self.actions.get(node.path)
        if action is DecisionAction.REMOVE_SUBTREE:
            return False
        if not node.children:
            return action is DecisionAction.KEEP and node.media_type in TEXT_TYPES
        return any(self._has_retained_leaf(child) for child in node.children)

    def _remove(self, node: RawNode, reason: RemovalReason) -> None:
        """Register one opaque deletion root."""
        self.actions[node.path] = DecisionAction.REMOVE_SUBTREE
        self.removals.append(Removal(node.path, node.media_type, reason))

    def _body(self, node: RawNode) -> None:
        """Classify one node in its container-owned body role.

        Raises:
            AppError: If its context, disposition, or media role is ambiguous.

        """
        disposition = _disposition(node)
        if node.media_type in PROTECTED_TYPES:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE, "protected MIME content", node.path
            )
        if disposition not in {None, "inline", "attachment"}:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "unknown MIME disposition",
                node.path,
            )
        if _has_filename(node) and disposition != "inline":
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "filename-only body role is ambiguous",
                node.path,
            )
        if node.media_type in TEXT_TYPES:
            self._text(node)
        elif node.media_type == "multipart/alternative":
            self._alternative(node)
        elif node.media_type == "multipart/related":
            self._related(node)
        elif node.media_type == "multipart/mixed":
            self._mixed(node)
        else:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                f"unsupported body MIME type {node.media_type}",
                node.path,
            )

    def _text(self, node: RawNode) -> None:
        """Keep a supported body representation only in a body context.

        Raises:
            AppError: If attachment metadata conflicts with the body role.

        """
        disposition = _disposition(node)
        if disposition == "attachment":
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "attachment body role conflict",
                node.path,
            )
        self.actions[node.path] = DecisionAction.KEEP

    def _alternative(self, node: RawNode) -> None:
        """Preserve all ordered alternative representations.

        Raises:
            AppError: If a representation is unsupported or role-conflicting.

        """
        if _disposition(node) == "attachment":
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "attachment alternative role conflict",
                node.path,
            )
        if not node.children:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "empty multipart/alternative",
                node.path,
            )
        self.actions[node.path] = DecisionAction.RECURSE
        for child in node.children:
            if child.media_type not in TEXT_TYPES | {
                "multipart/alternative",
                "multipart/related",
            }:
                raise AppError(
                    ExitCode.TRANSFORMATION_UNAVAILABLE,
                    "unsupported multipart/alternative branch",
                    child.path,
                )
            self._body(child)

    def _related(self, node: RawNode) -> None:
        """Keep exactly one related root and prune each direct non-root component.

        Raises:
            AppError: If root selection or related metadata is role-ambiguous.

        """
        if _disposition(node) == "attachment":
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "attachment related role conflict",
                node.path,
            )
        if not node.children:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "empty multipart/related",
                node.path,
            )
        ids, root = self._related_root(node)
        self._validate_related_type(node, root)
        self._validate_start_info(node, root, ids)
        self.actions[node.path] = DecisionAction.RECURSE
        self._body(root)
        for child in node.children:
            if child is not root:
                self._remove(child, RemovalReason.RELATED_NONROOT_COMPONENT)

    @staticmethod
    def _within(node: RawNode, root: RawNode) -> bool:
        """Return whether a node lies in the retained root subtree.

        Returns:
            Whether the node MIME path has the selected root path as its prefix.

        """
        return node.path[: len(root.path)] == root.path

    @staticmethod
    def _related_root(node: RawNode) -> tuple[dict[bytes, RawNode], RawNode]:
        """Resolve the direct related root.

        Returns:
            The Content-ID map and exactly selected direct root.

        Raises:
            AppError: If start does not identify one direct child.

        """
        ids = _related_ids(node)
        start = node.parameter(b"start")
        root = node.children[0] if start is None else ids.get(_parse_start(start))
        if root is None or root not in node.children:
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "related start has no direct root",
                node.path,
            )
        return ids, root

    @staticmethod
    def _validate_related_type(node: RawNode, root: RawNode) -> None:
        """Validate an optional related type against the selected root.

        Raises:
            AppError: If the declared type differs from the root media type.

        """
        declared_type = node.parameter(b"type")
        if (
            declared_type is not None
            and declared_type.lower() != root.media_type.encode("ascii")
        ):
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "related type does not match root",
                node.path,
            )

    def _validate_start_info(
        self, node: RawNode, root: RawNode, ids: dict[bytes, RawNode]
    ) -> None:
        """Require CID-form start-info targets to remain below the selected root.

        Raises:
            AppError: If start-info names a missing or removed component.

        """
        start_info = node.parameter(b"start-info")
        identifiers = () if start_info is None else _start_info_ids(start_info) or ()
        for identifier in identifiers:
            target = ids.get(identifier)
            if target is None or not self._within(target, root):
                raise AppError(
                    ExitCode.TRANSFORMATION_UNAVAILABLE,
                    "related start-info does not target a retained component",
                    node.path,
                )

    def _mixed(self, node: RawNode) -> None:
        """Apply explicit-attachment-only pruning to a multipart/mixed child list.

        Raises:
            AppError: If the mixed container itself is attachment-role conflicting.

        """
        if _disposition(node) == "attachment":
            raise AppError(
                ExitCode.TRANSFORMATION_UNAVAILABLE,
                "attachment mixed role conflict",
                node.path,
            )
        self.actions[node.path] = DecisionAction.RECURSE
        for child in node.children:
            disposition = _disposition(child)
            if disposition == "attachment":
                self._remove(child, RemovalReason.EXPLICIT_ATTACHMENT)
            else:
                self._body(child)


def classify(root: RawNode) -> PolicyResult:
    """Return the only deletion plan permitted by the v3 MIME policy.

    Returns:
        Closed contextual decisions and permitted removal roots.

    """
    return _Classifier().classify(root)

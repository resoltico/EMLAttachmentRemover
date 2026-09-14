"""Independent, source-structural authorization of verifier removal claims."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Final

from .domain import AppError, ExitCode, Removal, RemovalReason
from .mime_identifiers import (
    first_non_cfws,
    parse_message_identifier,
    parse_message_identifier_sequence,
)

if TYPE_CHECKING:
    from .mime_raw import RawMimeTree, RawNode

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


@dataclass(slots=True)
class _SourceAuthorization:
    """Own independently derived claims and the retained-body proof."""

    removals: list[Removal]

    def authorize(self, root: RawNode, claims: tuple[Removal, ...]) -> None:
        """Derive the exact permitted claims and compare the full typed set.

        Raises:
            AppError: If source policy is unsafe or supplied claims disagree.

        """
        if not self._body(root):
            raise AppError(ExitCode.VERIFICATION_ERROR, "no supported body remains")
        expected = tuple(self.removals)
        if claims != expected:
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "removal authorization mismatch"
            )

    def _body(self, node: RawNode) -> bool:
        """Authorize one retained body role without planner classifications.

        Returns:
            Whether this source subtree retains a supported text body leaf.

        """
        disposition = self._common_role(node)
        if node.media_type in TEXT_TYPES:
            return self._text(disposition)
        return self._container(node, disposition)

    @staticmethod
    def _common_role(node: RawNode) -> str | None:
        """Validate source facts shared by every body-context node.

        Returns:
            The normalized source disposition token, when present.

        Raises:
            AppError: If a protected, unknown, or filename-only role is present.

        """
        disposition = _disposition(node)
        if node.media_type in PROTECTED_TYPES:
            raise AppError(ExitCode.VERIFICATION_ERROR, "protected MIME content")
        if disposition not in {None, "inline", "attachment"}:
            raise AppError(ExitCode.VERIFICATION_ERROR, "unknown MIME disposition")
        if _has_filename(node) and disposition != "inline":
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "filename-only body role is ambiguous"
            )
        return disposition

    @staticmethod
    def _text(disposition: str | None) -> bool:
        """Authorize one supported text leaf.

        Returns:
            Always ``True`` for the retained body leaf.

        Raises:
            AppError: If attachment metadata conflicts with its text body role.

        """
        if disposition == "attachment":
            raise AppError(ExitCode.VERIFICATION_ERROR, "attachment body role conflict")
        return True

    def _container(self, node: RawNode, disposition: str | None) -> bool:
        """Dispatch an independently checked multipart container role.

        Returns:
            Whether the selected container retains a supported text body leaf.

        Raises:
            AppError: If the source container type is unsupported.

        """
        if node.media_type == "multipart/alternative":
            return self._alternative(node, disposition)
        if node.media_type == "multipart/mixed":
            return self._mixed(node, disposition)
        if node.media_type == "multipart/related":
            return self._related(node, disposition)
        raise AppError(
            ExitCode.VERIFICATION_ERROR, f"unsupported body MIME type {node.media_type}"
        )

    def _alternative(self, node: RawNode, disposition: str | None) -> bool:
        """Authorize every supported alternative representation in source order.

        Returns:
            Whether all source-ordered retained children include a text body leaf.

        Raises:
            AppError: If the alternative role or a branch is unsupported.

        """
        if disposition == "attachment":
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "attachment alternative role conflict"
            )
        if not node.children:
            raise AppError(ExitCode.VERIFICATION_ERROR, "empty multipart/alternative")
        allowed = TEXT_TYPES | {"multipart/alternative", "multipart/related"}
        if any(child.media_type not in allowed for child in node.children):
            raise AppError(
                ExitCode.VERIFICATION_ERROR,
                "unsupported multipart/alternative branch",
            )
        retained = False
        for child in node.children:
            retained = self._body(child) or retained
        return retained

    def _mixed(self, node: RawNode, disposition: str | None) -> bool:
        """Authorize explicit direct attachment roots and recurse into other children.

        Returns:
            Whether the nonattachment children retain a supported text body leaf.

        Raises:
            AppError: If the mixed container itself is attachment-role conflicting.

        """
        if disposition == "attachment":
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "attachment mixed role conflict"
            )
        retained = False
        for child in node.children:
            if _disposition(child) == "attachment":
                self.removals.append(
                    Removal(
                        child.path, child.media_type, RemovalReason.EXPLICIT_ATTACHMENT
                    )
                )
            else:
                retained = self._body(child) or retained
        return retained

    def _related(self, node: RawNode, disposition: str | None) -> bool:
        """Authorize the selected related root and its direct nonroot removals.

        Returns:
            Whether the independently selected related root retains a text leaf.

        Raises:
            AppError: If the related container itself is attachment-role conflicting.

        """
        if disposition == "attachment":
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "attachment related role conflict"
            )
        root, identifiers = _related_root(node)
        _validate_related_controls(node, root, identifiers)
        retained = self._body(root)
        for child in node.children:
            if child is not root:
                self.removals.append(
                    Removal(
                        child.path,
                        child.media_type,
                        RemovalReason.RELATED_NONROOT_COMPONENT,
                    )
                )
        return retained


def authorize_removals(tree: RawMimeTree, claims: tuple[Removal, ...]) -> None:
    """Independently authorize an exact ordered tuple of source removal claims."""
    _SourceAuthorization([]).authorize(tree.root, claims)


def _disposition(node: RawNode) -> str | None:
    """Return one source-declared normalized disposition token.

    Returns:
        The source disposition token, or ``None`` when it is absent.

    """
    return None if node.disposition is None else node.disposition.token


def _has_filename(node: RawNode) -> bool:
    """Return whether source metadata creates a filename-only ambiguous body role.

    Returns:
        Whether a filename/name control is present.

    """
    return b"name" in node.content_type.parameters or (
        node.disposition is not None and b"filename" in node.disposition.parameters
    )


def _related_root(node: RawNode) -> tuple[RawNode, dict[bytes, RawNode]]:
    """Select one direct related root using independently parsed source IDs.

    Returns:
        The direct selected root and all unique identifiers in the compound.

    Raises:
        AppError: If the compound lacks children or a valid direct selected root.

    """
    if not node.children:
        raise AppError(ExitCode.VERIFICATION_ERROR, "empty multipart/related")
    identifiers = _related_identifiers(node)
    start = node.parameter(b"start")
    root = (
        node.children[0] if start is None else identifiers.get(_start_identifier(start))
    )
    if root is None or root not in node.children:
        raise AppError(ExitCode.VERIFICATION_ERROR, "related start has no direct root")
    return root, identifiers


def _related_identifiers(node: RawNode) -> dict[bytes, RawNode]:
    """Return exact unique identifiers from the entire source related compound.

    Returns:
        Identifiers mapped to their unique source nodes.

    Raises:
        AppError: If a nested source component duplicates an identifier.

    """
    identifiers: dict[bytes, RawNode] = {}
    pending = list(node.children)
    while pending:
        current = pending.pop()
        identifier = _content_identifier(current)
        if identifier is not None:
            if identifier in identifiers:
                raise AppError(
                    ExitCode.VERIFICATION_ERROR, "duplicate canonical Content-ID"
                )
            identifiers[identifier] = current
        pending.extend(current.children)
    return identifiers


def _content_identifier(node: RawNode) -> bytes | None:
    """Parse the source's one prevalidated Content-ID without planner facts.

    Returns:
        The exact inner identifier, or ``None`` if source supplies no Content-ID.

    """
    values = node.header_map.get(b"content-id")
    return (
        None
        if not values
        else parse_message_identifier(values[0].value, field="Content-ID")
    )


def _start_identifier(value: bytes) -> bytes:
    """Parse one exact source related-start identifier.

    Returns:
        The exact inner source identifier.

    """
    return parse_message_identifier(value, field="multipart/related start")


def _validate_related_controls(
    node: RawNode, root: RawNode, identifiers: dict[bytes, RawNode]
) -> None:
    """Validate related type and CID-form start-info against the retained root.

    Raises:
        AppError: If source controls select a missing or removed component.

    """
    declared_type = node.parameter(b"type")
    if declared_type is not None and declared_type.lower() != root.media_type.encode():
        raise AppError(ExitCode.VERIFICATION_ERROR, "related type does not match root")
    start_info = node.parameter(b"start-info")
    if start_info is None:
        return
    start = first_non_cfws(start_info)
    if start == len(start_info) or start_info[start] != OPEN_ANGLE:
        return
    targets = parse_message_identifier_sequence(start_info, field="related start-info")
    if len(set(targets)) != len(targets) or any(
        target not in identifiers or not _within(identifiers[target], root)
        for target in targets
    ):
        raise AppError(
            ExitCode.VERIFICATION_ERROR,
            "related start-info does not target a retained component",
        )


def _within(node: RawNode, root: RawNode) -> bool:
    """Return whether one source node lies within the selected retained root.

    Returns:
        Whether the candidate node path has the root path as a prefix.

    """
    return node.path[: len(root.path)] == root.path

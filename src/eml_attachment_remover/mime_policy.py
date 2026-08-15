"""Apply the safe MIME attachment-removal policy."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

from .mime_body import (
    ATTACHMENT_DISPOSITION,
    BODY_CONTAINER_TYPES,
    BODY_TEXT_TYPES,
    INLINE_DISPOSITION,
    _replace_root_payload,
)
from .mime_locations import (
    CONTENT_LOCATION_HEADER,
    _content_location_base,
    _normalize_content_location_header,
    _resource_location_variants,
)
from .mime_references import (
    CONTENT_ID_HEADER,
    _collect_references,
    _is_email_message_list,
    _normalize_content_id_header,
)
from .models import (
    CliError,
    ExitCode,
    KeepReason,
    MimePath,
    PartContext,
    PreservedFilePart,
    ReferenceIndex,
    RemovalState,
    RemovedPart,
    _format_mime_path,
)

if TYPE_CHECKING:
    from email.message import EmailMessage, Message

PROTECTED_MULTIPART_TYPES: Final = frozenset(
    {"multipart/encrypted", "multipart/signed"},
)
OPAQUE_SECURITY_TYPES: Final = frozenset(
    {
        "application/pkcs7-mime",
        "application/x-pkcs7-mime",
        "application/pgp-encrypted",
    },
)
CONTENT_DIGEST_HEADER: Final = "Content-MD5"


def _referenced_keep_reason(
    content_id: str | None,
    location_variants: frozenset[str],
    references: ReferenceIndex,
) -> KeepReason | None:
    """Return a body-reference retention reason when one applies.

    Returns:
        The matching body-reference reason, or ``None``.

    """
    if content_id is not None and content_id in references.content_ids:
        return KeepReason.BODY_CID_REFERENCE
    if location_variants & references.locations:
        return KeepReason.BODY_LOCATION_REFERENCE
    return None


def _metadata_keep_reason(
    part: EmailMessage,
    context: PartContext,
    content_id: str | None,
    location: str | None,
) -> KeepReason | None:
    """Return a non-body-reference retention reason when one applies.

    Returns:
        The matching metadata reason, or ``None``.

    """
    disposition = part.get_content_disposition()
    reason: KeepReason | None = None
    if part.get_content_type() in OPAQUE_SECURITY_TYPES | PROTECTED_MULTIPART_TYPES:
        reason = KeepReason.SECURITY_ENTITY
    elif disposition == ATTACHMENT_DISPOSITION:
        return reason
    elif disposition == INLINE_DISPOSITION:
        reason = KeepReason.INLINE_DISPOSITION
    elif context.under_related:
        reason = KeepReason.RELATED_RESOURCE
    elif content_id is not None:
        reason = KeepReason.CONTENT_ID
    elif location is not None:
        reason = KeepReason.CONTENT_LOCATION
    return reason


def _keep_reason(part: EmailMessage, context: PartContext) -> KeepReason | None:
    """Return the reason that protects a MIME entity from removal.

    Returns:
        A retention reason, or ``None`` when the entity is removable.

    """
    content_id = _normalize_content_id_header(part.get(CONTENT_ID_HEADER))
    location = _normalize_content_location_header(part.get(CONTENT_LOCATION_HEADER))
    return _referenced_keep_reason(
        content_id,
        _resource_location_variants(part, context.location_base),
        context.references,
    ) or _metadata_keep_reason(part, context, content_id, location)


def _is_file_like(part: EmailMessage) -> bool:
    """Return whether a MIME entity exposes file-like metadata.

    Returns:
        ``True`` for named, inline, or attachment-disposition entities.

    """
    return part.get_filename() is not None or part.get_content_disposition() in {
        ATTACHMENT_DISPOSITION,
        INLINE_DISPOSITION,
    }


def _should_remove(part: EmailMessage, context: PartContext) -> bool:
    """Return whether the attachment policy removes one non-root MIME entity.

    Returns:
        ``True`` when the entity is a removable attachment.

    """
    if context.is_root or _keep_reason(part, context) is not None:
        return False
    if part.get_content_disposition() == ATTACHMENT_DISPOSITION:
        return True
    if part.get_filename() is None:
        return False
    return not (
        part.get_content_type() in BODY_TEXT_TYPES
        and context.parent_type in BODY_CONTAINER_TYPES
    )


def _root_is_removable(part: EmailMessage, references: ReferenceIndex) -> bool:
    """Return whether the root MIME entity is itself an attachment payload.

    Returns:
        ``True`` when the root should be replaced by a removal notice.

    """
    disposition = part.get_content_disposition()
    if disposition == ATTACHMENT_DISPOSITION:
        return True
    context = _root_context(references)
    if _keep_reason(part, context) is not None:
        return False
    return (
        part.get_filename() is not None
        and part.get_content_type() not in BODY_TEXT_TYPES
    )


def _record_preserved_file_part(
    part: EmailMessage,
    context: PartContext,
    state: RemovalState,
) -> None:
    """Record a file-like MIME entity retained as body or protected content."""
    reason = _keep_reason(part, context)
    if reason is None:
        return
    if reason is KeepReason.SECURITY_ENTITY:
        state.protected_types.add(part.get_content_type())
    if not _is_file_like(part):
        return
    state.preserved_file_parts.append(
        PreservedFilePart(
            path=context.path,
            content_type=part.get_content_type(),
            filename=part.get_filename(),
            reason=reason,
        ),
    )


def _removed_part(part: EmailMessage, path: MimePath) -> RemovedPart:
    """Create a removal record for one MIME entity.

    Returns:
        The immutable removal record.

    """
    return RemovedPart(
        path=path,
        content_type=part.get_content_type(),
        filename=part.get_filename(),
        disposition=part.get_content_disposition(),
    )


def _child_context(
    context: PartContext,
    parent: EmailMessage,
    index: int,
) -> PartContext:
    """Return traversal context for one direct child.

    Returns:
        Context inheriting the parent path, relationship, and references.

    """
    parent_type = parent.get_content_type()
    return PartContext(
        path=(*context.path, index),
        parent_type=parent_type,
        under_related=context.under_related or parent_type == "multipart/related",
        references=context.references,
        location_base=_content_location_base(parent, context.location_base),
    )


def _container_context(part: EmailMessage, context: PartContext) -> PartContext:
    """Narrow references to the current related aggregate or nested message.

    Returns:
        Context scoped to the aggregate, or the original context otherwise.

    """
    if (
        part.get_content_type() != "multipart/related"
        and part.get_content_maintype() != "message"
    ):
        return context
    return PartContext(
        path=context.path,
        parent_type=context.parent_type,
        under_related=context.under_related,
        references=_collect_references(part, inherited_base=context.location_base),
        location_base=context.location_base,
        is_root=context.is_root,
    )


def _first_removable_in_subtree(
    part: EmailMessage,
    context: PartContext,
) -> RemovedPart | None:
    """Find the first removable entity without mutating the MIME tree.

    Returns:
        The first removal record, or ``None`` when none exists.

    """
    if _should_remove(part, context):
        return _removed_part(part, context.path)
    if not part.is_multipart():
        return None
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        return None
    parent_type = part.get_content_type()
    if parent_type == "multipart/encrypted":
        return None
    if parent_type == "multipart/signed":
        payload = payload[:1]
    context = _container_context(part, context)
    for index, raw_child in enumerate(payload):
        found = _first_removable_in_subtree(
            raw_child,
            _child_context(context, part, index),
        )
        if found is not None:
            return found
    return None


def _guard_signed_container(part: EmailMessage, context: PartContext) -> None:
    """Refuse to alter an attachment covered by ``multipart/signed``.

    Raises:
        CliError: If signed content contains a removable attachment.

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload) or not payload:
        return
    found = _first_removable_in_subtree(
        payload[0],
        _child_context(context, part, 0),
    )
    if found is None:
        return
    name = found.filename or "unnamed MIME entity"
    message = (
        f"attachment {name!r} at MIME path {_format_mime_path(found.path)} is "
        "inside multipart/signed content; refusing to invalidate the MIME signature"
    )
    raise CliError(ExitCode.PROTECTED_MESSAGE, message)


def _clear_content_digest(part: EmailMessage) -> None:
    """Remove a stale per-part body digest after the payload changes."""
    if CONTENT_DIGEST_HEADER in part:
        del part[CONTENT_DIGEST_HEADER]


def _handle_protected_parent(
    parent: EmailMessage,
    context: PartContext,
    state: RemovalState,
) -> bool:
    """Record and retain protected MIME containers.

    Returns:
        ``True`` when the parent must not be traversed or modified.

    """
    parent_type = parent.get_content_type()
    if parent_type == "multipart/signed":
        state.protected_types.add(parent_type)
        _guard_signed_container(parent, context)
        return True
    if parent_type == "multipart/encrypted":
        state.protected_types.add(parent_type)
        return True
    return False


def _is_empty_multipart(part: EmailMessage) -> bool:
    """Return whether a multipart entity has no retained children.

    Returns:
        ``True`` when the entity is a multipart with an empty payload list.

    """
    payload = part.get_payload()
    return part.is_multipart() and isinstance(payload, list) and not payload


def _remove_children(
    parent: EmailMessage,
    context: PartContext,
    state: RemovalState,
) -> bool:
    """Remove attachment entities recursively from one multipart container.

    Returns:
        ``True`` when the container or one of its descendants changed.

    """
    if not parent.is_multipart() or _handle_protected_parent(parent, context, state):
        return False
    payload = parent.get_payload()
    if not _is_email_message_list(payload):
        return False
    context = _container_context(parent, context)
    kept: list[EmailMessage] = []
    modified = False
    for index, child in enumerate(payload):
        child_context = _child_context(context, parent, index)
        if _should_remove(child, child_context):
            state.removed.append(_removed_part(child, child_context.path))
            modified = True
            continue
        _record_preserved_file_part(child, child_context, state)
        modified = _remove_children(child, child_context, state) or modified
        if _is_empty_multipart(child):
            modified = True
            continue
        kept.append(child)
    if modified:
        retained_payload: list[Message[str, str] | str] = []
        retained_payload.extend(kept)
        parent.set_payload(retained_payload)
        _clear_content_digest(parent)
    return modified


def _root_context(references: ReferenceIndex) -> PartContext:
    """Return the traversal context for a message root.

    Returns:
        The root context containing the supplied body references.

    """
    return PartContext(
        path=(),
        parent_type=None,
        under_related=False,
        references=references,
        is_root=True,
    )


def _remove_attachments(
    message: EmailMessage,
    references: ReferenceIndex,
) -> RemovalState:
    """Remove removable MIME entities from a parsed message.

    Returns:
        The accumulated removal state.

    Raises:
        CliError: If safe removal would alter signed MIME content.

    """
    state = RemovalState()
    root_context = _root_context(references)
    root_type = message.get_content_type()
    if root_type == "multipart/signed":
        state.protected_types.add(root_type)
        _guard_signed_container(message, root_context)
        return state
    if root_type == "multipart/encrypted" or root_type in OPAQUE_SECURITY_TYPES:
        state.protected_types.add(root_type)
        return state
    if _root_is_removable(message, references):
        state.removed.append(_removed_part(message, ()))
        _replace_root_payload(
            message,
            "[The original root attachment was removed from this derived EML copy.]\n",
        )
        return state
    _record_preserved_file_part(message, root_context, state)
    modified = _remove_children(message, root_context, state)
    if state.removed and "multipart/signed" in state.protected_types:
        raise CliError(
            ExitCode.PROTECTED_MESSAGE,
            "message contains multipart/signed content and removable attachments; "
            "refusing to reserialize because the MIME signature could be invalidated",
        )
    if modified and _is_empty_multipart(message):
        _replace_root_payload(
            message,
            "[All MIME body parts were attachments and were removed from this "
            "derived EML copy.]\n",
        )
    return state

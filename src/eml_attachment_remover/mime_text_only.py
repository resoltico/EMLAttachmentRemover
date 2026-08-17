"""Plan, execute, and verify the canonical plain-text MIME transformation."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING

from .mime_body import ATTACHMENT_DISPOSITION, INLINE_DISPOSITION
from .mime_locations import (
    CONTENT_LOCATION_HEADER,
    _content_location_base,
)
from .mime_references import (
    CONTENT_ID_HEADER,
    _is_email_message_list,
)
from .mime_text_execution import canonical_text_payload
from .mime_text_resources import (
    PROTECTED_MIME_TYPES,
    ReferenceSource,
    RootInclusion,
)
from .mime_text_resources import (
    ResourceScan as _ResourceScan,
)
from .mime_text_resources import (
    html_reference_sources as _html_reference_sources,
)
from .mime_text_resources import (
    resource_leaves as _resource_leaves,
)
from .mime_text_selection import (
    can_select_plain as _can_select_plain,
)
from .mime_text_selection import (
    is_resource_free_plain as _is_resource_free_plain,
)
from .mime_text_selection import (
    is_safe_body_container as _is_safe_body_container,
)
from .mime_text_selection import (
    related_root_index as _related_root_index,
)
from .models import (
    CliError,
    DiscardedBodyRepresentation,
    DiscardedBodyResource,
    ExitCode,
    MimePath,
    RemovedPart,
    SelectedPlainTextBody,
    _format_mime_path,
)

if TYPE_CHECKING:
    from email.message import EmailMessage


@dataclass(frozen=True, slots=True)
class TextOnlyPlan:
    """Store the complete immutable transformation plan at source-tree paths."""

    selected_body: SelectedPlainTextBody
    selected_payload_sha256: str
    removed_attachments: tuple[RemovedPart, ...] = ()
    discarded_representations: tuple[DiscardedBodyRepresentation, ...] = ()
    discarded_resources: tuple[DiscardedBodyResource, ...] = ()
    discard_paths: tuple[MimePath, ...] = ()

    @property
    def modified(self) -> bool:
        """Whether execution must rewrite the root message.

        Returns:
            ``True`` for body promotion or any planned discard.

        """
        return bool(self.selected_body.path or self.discard_paths)

    @property
    def changed_paths(self) -> tuple[MimePath, ...]:
        """Every source path proving that the root message changed.

        Returns:
            Discard roots plus the promoted body path when it was nested.

        """
        if not self.selected_body.path:
            return self.discard_paths
        return (*self.discard_paths, self.selected_body.path)


def _transformation_unavailable(path: MimePath, detail: str) -> CliError:
    """Return the stable fail-closed text-only transformation error.

    Returns:
        An expected exit-code-six error naming the unsafe MIME path.

    """
    return CliError(
        ExitCode.TRANSFORMATION_UNAVAILABLE,
        "cannot produce a text-only EML: "
        f"{detail} at MIME path {_format_mime_path(path)}",
    )


def _selected_plain(part: EmailMessage, path: MimePath) -> TextOnlyPlan:
    """Bind one selected plain-text leaf to its original decoded payload.

    Returns:
        A plan containing its public identity and private SHA-256 precondition.

    """
    return TextOnlyPlan(
        selected_body=SelectedPlainTextBody(path, part.get_content_type()),
        selected_payload_sha256=hashlib.sha256(
            canonical_text_payload(part),
        ).hexdigest(),
    )


def _extend_plan(
    plan: TextOnlyPlan,
    *,
    attachments: tuple[RemovedPart, ...] = (),
    representations: tuple[DiscardedBodyRepresentation, ...] = (),
    resources: tuple[DiscardedBodyResource, ...] = (),
    discard_paths: tuple[MimePath, ...] = (),
) -> TextOnlyPlan:
    """Return a plan extended with complete immutable discard actions.

    Returns:
        A new plan preserving every action and audit category.

    """
    return TextOnlyPlan(
        selected_body=plan.selected_body,
        selected_payload_sha256=plan.selected_payload_sha256,
        removed_attachments=(*plan.removed_attachments, *attachments),
        discarded_representations=(
            *plan.discarded_representations,
            *representations,
        ),
        discarded_resources=(*plan.discarded_resources, *resources),
        discard_paths=(*plan.discard_paths, *discard_paths),
    )


def _plan_alternative(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> TextOnlyPlan:
    """Select the sole safe plain alternative and discard every other branch.

    Returns:
        The complete plan for this alternative aggregate.

    Raises:
        _transformation_unavailable: If there is not exactly one eligible plain
            representation.

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        raise _transformation_unavailable(
            path,
            "multipart/alternative has no traversable representations",
        )
    candidates = [
        index for index, child in enumerate(payload) if _is_resource_free_plain(child)
    ]
    if len(candidates) != 1:
        raise _transformation_unavailable(
            path,
            "multipart/alternative does not contain exactly one resource-free "
            "text/plain representation",
        )
    selected_index = candidates[0]
    selected_path = (*path, selected_index)
    plan = _selected_plain(payload[selected_index], selected_path)
    child_base = _content_location_base(part, inherited_base)
    for index, child in enumerate(payload):
        if index == selected_index:
            continue
        child_path = (*path, index)
        sources = _html_reference_sources(child, child_path, child_base)
        plan = _extend_plan(
            plan,
            representations=(
                DiscardedBodyRepresentation(child_path, child.get_content_type()),
            ),
            resources=_resource_leaves(
                child,
                child_path,
                _ResourceScan(
                    reference_sources=sources,
                    inherited_base=child_base,
                    include_text=False,
                    root_inclusion=RootInclusion.OMIT,
                ),
            ),
            discard_paths=(child_path,),
        )
    return plan


def _plan_related(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> TextOnlyPlan:
    """Retain the related root's plain body and discard its resource closure.

    Returns:
        The body plan extended with every related sibling resource.

    Raises:
        _transformation_unavailable: If the related aggregate has no resolvable
            root.

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        raise _transformation_unavailable(
            path,
            "multipart/related has no traversable root entity",
        )
    root_index = _related_root_index(part, payload)
    if root_index is None:
        raise _transformation_unavailable(
            path,
            "multipart/related does not resolve one unique root entity",
        )
    child_base = _content_location_base(part, inherited_base)
    root = payload[root_index]
    declared_type = part.get_param("type")
    if declared_type is not None and str(declared_type).casefold() != (
        root.get_content_type().casefold()
    ):
        raise _transformation_unavailable(
            path,
            "multipart/related type does not match its resolved root entity",
        )
    plan = _plan_body(root, (*path, root_index), child_base)
    sources = _html_reference_sources(root, (*path, root_index), child_base)
    for index, child in enumerate(payload):
        if index == root_index:
            continue
        child_path = (*path, index)
        plan = _extend_plan(
            plan,
            resources=_resource_leaves(
                child,
                child_path,
                _ResourceScan(
                    reference_sources=sources,
                    inherited_base=child_base,
                    include_text=True,
                    root_inclusion=RootInclusion.INCLUDE,
                ),
            ),
            discard_paths=(child_path,),
        )
    return plan


def _plan_mixed_sibling(
    plan: TextOnlyPlan,
    child: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
    sources: tuple[ReferenceSource, ...],
) -> TextOnlyPlan:
    """Classify one non-body mixed sibling into exactly one discard category.

    Returns:
        The plan extended with one atomic sibling discard.

    Raises:
        _transformation_unavailable: If the sibling's role is ambiguous.

    """
    disposition = child.get_content_disposition()
    is_attachment = disposition == ATTACHMENT_DISPOSITION or (
        child.get_filename() is not None and disposition != INLINE_DISPOSITION
    )
    has_resource_identity = (
        disposition == INLINE_DISPOSITION
        or child.get(CONTENT_ID_HEADER) is not None
        or child.get(CONTENT_LOCATION_HEADER) is not None
    )
    resources = (
        _resource_leaves(
            child,
            path,
            _ResourceScan(
                reference_sources=sources,
                inherited_base=inherited_base,
                include_text=True,
                root_inclusion=RootInclusion.INCLUDE,
            ),
        )
        if has_resource_identity
        else ()
    )
    if has_resource_identity and (
        not is_attachment or any(resource.referenced_by for resource in resources)
    ):
        return _extend_plan(
            plan,
            resources=resources,
            discard_paths=(path,),
        )
    protected = child.get_content_type() in PROTECTED_MIME_TYPES
    is_unnamed_binary = (
        child.get_content_maintype() not in {"message", "multipart", "text"}
        and not protected
    )
    if is_attachment or is_unnamed_binary:
        return _extend_plan(
            plan,
            attachments=(
                RemovedPart(
                    path,
                    child.get_content_type(),
                    child.get_filename(),
                    disposition,
                ),
            ),
            discard_paths=(path,),
        )
    raise _transformation_unavailable(
        path,
        f"{child.get_content_type()} has an ambiguous non-body role",
    )


def _plan_mixed(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> TextOnlyPlan:
    """Select one mixed body and classify every sibling before mutation.

    Returns:
        A whole-container plan with no unclassified retained sibling.

    Raises:
        _transformation_unavailable: If a unique body cannot be selected or a
            sibling role is ambiguous.

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        raise _transformation_unavailable(
            path,
            "multipart/mixed has no traversable body entity",
        )
    candidates = [
        index for index, child in enumerate(payload) if _can_select_plain(child)
    ]
    if len(candidates) != 1:
        raise _transformation_unavailable(
            path,
            "multipart/mixed does not contain one unique plain-text body",
        )
    selected_index = candidates[0]
    child_base = _content_location_base(part, inherited_base)
    selected_path = (*path, selected_index)
    selected = payload[selected_index]
    sources = _html_reference_sources(selected, selected_path, child_base)
    plan = _plan_body(selected, selected_path, child_base)
    for index, child in enumerate(payload):
        if index == selected_index:
            continue
        child_path = (*path, index)
        plan = _plan_mixed_sibling(
            plan,
            child,
            child_path,
            child_base,
            sources,
        )
    return plan


def _plan_body(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> TextOnlyPlan:
    """Plan the canonical plain-text representation of one logical body.

    Returns:
        One immutable whole-transformation plan.

    Raises:
        _transformation_unavailable: If the body cannot reduce safely to plain
            text.

    """
    if _is_resource_free_plain(part):
        return _selected_plain(part, path)
    content_type = part.get_content_type()
    if not _is_safe_body_container(part):
        raise _transformation_unavailable(
            path,
            f"{content_type} body container has file-like metadata",
        )
    if content_type == "multipart/alternative":
        return _plan_alternative(part, path, inherited_base)
    if content_type == "multipart/related":
        return _plan_related(part, path, inherited_base)
    if content_type == "multipart/mixed":
        return _plan_mixed(part, path, inherited_base)
    protected = "protected " if content_type in PROTECTED_MIME_TYPES else ""
    raise _transformation_unavailable(
        path,
        f"{protected}MIME entity {content_type!r} is not a safe plain-text body",
    )


def _plan_text_only(message: EmailMessage) -> TextOnlyPlan:
    """Analyze a complete source tree without mutation.

    Returns:
        A source-path-sorted immutable plan covering the entire transformation.

    """
    plan = _plan_body(message, (), None)
    return TextOnlyPlan(
        selected_body=plan.selected_body,
        selected_payload_sha256=plan.selected_payload_sha256,
        removed_attachments=tuple(
            sorted(plan.removed_attachments, key=lambda item: item.path),
        ),
        discarded_representations=tuple(
            sorted(plan.discarded_representations, key=lambda item: item.path),
        ),
        discarded_resources=tuple(
            sorted(plan.discarded_resources, key=lambda item: item.path),
        ),
        discard_paths=tuple(sorted(plan.discard_paths)),
    )

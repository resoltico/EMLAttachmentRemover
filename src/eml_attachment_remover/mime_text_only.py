"""Plan, execute, and verify the canonical plain-text MIME transformation."""

from __future__ import annotations

from typing import TYPE_CHECKING, NoReturn

from .html_text import equivalent_html_layout_source
from .mime_body import ATTACHMENT_DISPOSITION, INLINE_DISPOSITION
from .mime_locations import CONTENT_LOCATION_HEADER, _content_location_base
from .mime_references import CONTENT_ID_HEADER, _is_email_message_list
from .mime_text_plan import TextOnlyPlan, transformation_unavailable
from .mime_text_plan import (
    extend_plan as _extend_plan,
)
from .mime_text_plan import (
    selected_plain as _selected_plain,
)
from .mime_text_plan import (
    sorted_plan as _sorted_plan,
)
from .mime_text_plan import (
    with_html_layout as _with_html_layout,
)
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
    DiscardedBodyRepresentation,
    MimePath,
    RemovedPart,
)

if TYPE_CHECKING:
    from email.message import EmailMessage


__all__ = ["TextOnlyPlan"]

_transformation_unavailable = transformation_unavailable


def _raise_unavailable(path: MimePath, detail: str) -> NoReturn:
    """Raise one stable fail-closed text-only transformation error.

    Raises:
        transformation_unavailable: Always, with an exit-code-six diagnostic.

    """
    raise transformation_unavailable(path, detail)


def html_representation_text(part: EmailMessage) -> str | None:
    """Return decoded HTML from one direct or related body representation.

    Returns:
        Text only when the representation resolves to one safe HTML body.

    """
    if part.get_content_type() == "text/html" and _is_safe_body_container(part):
        text = part.get_content()
        return text if isinstance(text, str) else None
    if part.get_content_type() != "multipart/related" or not _is_safe_body_container(
        part,
    ):
        return None
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        return None
    root_index = _related_root_index(part, payload)
    if root_index is None:
        return None
    return html_representation_text(payload[root_index])


def _plan_alternative(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> TextOnlyPlan:
    """Select the sole safe plain alternative and discard every other branch.

    Returns:
        The complete plan for this alternative aggregate.

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        _raise_unavailable(
            path,
            "multipart/alternative has no traversable representations",
        )
    candidates = [
        index for index, child in enumerate(payload) if _is_resource_free_plain(child)
    ]
    if len(candidates) != 1:
        _raise_unavailable(
            path,
            "multipart/alternative does not contain exactly one resource-free "
            "text/plain representation",
        )
    selected_index = candidates[0]
    plan = _selected_plain(payload[selected_index], (*path, selected_index))
    plain_text = payload[selected_index].get_content()
    html_bodies = tuple(
        ((*path, index), text)
        for index, part in enumerate(payload)
        if index != selected_index
        and (text := html_representation_text(part)) is not None
    )
    layout = (
        equivalent_html_layout_source(plain_text, html_bodies)
        if isinstance(plain_text, str)
        else None
    )
    if layout is not None:
        layout_path, rendered_text = layout
        plan = _with_html_layout(
            plan,
            source=layout_path,
            rendered_text=rendered_text,
        )
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

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        _raise_unavailable(
            path,
            "multipart/related has no traversable root entity",
        )
    root_index = _related_root_index(part, payload)
    if root_index is None:
        _raise_unavailable(
            path,
            "multipart/related does not resolve one unique root entity",
        )
    child_base = _content_location_base(part, inherited_base)
    root = payload[root_index]
    declared_type = part.get_param("type")
    if declared_type is not None and str(declared_type).casefold() != (
        root.get_content_type().casefold()
    ):
        _raise_unavailable(
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
    _raise_unavailable(
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

    """
    payload = part.get_payload()
    if not _is_email_message_list(payload):
        _raise_unavailable(
            path,
            "multipart/mixed has no traversable body entity",
        )
    candidates = [
        index for index, child in enumerate(payload) if _can_select_plain(child)
    ]
    if len(candidates) != 1:
        _raise_unavailable(
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

    """
    if _is_resource_free_plain(part):
        return _selected_plain(part, path)
    content_type = part.get_content_type()
    if not _is_safe_body_container(part):
        _raise_unavailable(
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
    _raise_unavailable(
        path,
        f"{protected}MIME entity {content_type!r} is not a safe plain-text body",
    )


def _plan_text_only(message: EmailMessage) -> TextOnlyPlan:
    """Analyze a complete source tree without mutation.

    Returns:
        A source-path-sorted immutable plan covering the entire transformation.

    """
    return _sorted_plan(_plan_body(message, (), None))

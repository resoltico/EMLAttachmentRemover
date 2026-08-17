"""Audit scoped resources belonging to discarded MIME body representations."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, auto
from typing import TYPE_CHECKING, Final

from .mime_body import ATTACHMENT_DISPOSITION
from .mime_locations import (
    _content_location_base,
    _resource_location_variants,
)
from .mime_references import (
    CONTENT_ID_HEADER,
    _collect_references,
    _is_email_message_list,
    _normalize_content_id_header,
)
from .mime_text_selection import related_root_index
from .models import DiscardedBodyResource, MimePath, ReferenceIndex

if TYPE_CHECKING:
    from email.message import EmailMessage

type ReferenceSource = tuple[MimePath, ReferenceIndex]

PROTECTED_MIME_TYPES: Final = frozenset({
    "application/pgp-encrypted",
    "application/pkcs7-mime",
    "application/x-pkcs7-mime",
    "multipart/encrypted",
    "multipart/signed",
})


class RootInclusion(Enum):
    """Select whether the current scan root itself is an audit resource."""

    INCLUDE = auto()
    OMIT = auto()


@dataclass(frozen=True, slots=True)
class ResourceScan:
    """Bind scoped body references and location state for resource auditing."""

    reference_sources: tuple[ReferenceSource, ...]
    inherited_base: str | None
    include_text: bool
    root_inclusion: RootInclusion


def html_reference_sources(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
) -> tuple[ReferenceSource, ...]:
    """Collect each discarded HTML leaf's scoped resource references.

    Returns:
        Reference indexes paired with their original MIME paths.

    """

    def visit(
        current: EmailMessage,
        current_path: MimePath,
        current_base: str | None,
    ) -> tuple[ReferenceSource, ...]:
        content_type = current.get_content_type()
        if (
            current.get_content_maintype() == "message"
            or content_type in PROTECTED_MIME_TYPES
            or current.get_content_disposition() == ATTACHMENT_DISPOSITION
            or (current is not part and content_type == "multipart/related")
        ):
            return ()
        payload = current.get_payload()
        if _is_email_message_list(payload):
            child_base = _content_location_base(current, current_base)
            children: tuple[tuple[int, EmailMessage], ...]
            if content_type == "multipart/related":
                root_index = related_root_index(current, payload)
                if root_index is None:
                    return ()
                children = ((root_index, payload[root_index]),)
            else:
                children = tuple(enumerate(payload))
            return tuple(
                source
                for index, child in children
                for source in visit(
                    child,
                    (*current_path, index),
                    child_base,
                )
            )
        if content_type != "text/html":
            return ()
        return (
            (
                current_path,
                _collect_references(current, inherited_base=current_base),
            ),
        )

    return visit(part, path, inherited_base)


def _body_resource(
    part: EmailMessage,
    path: MimePath,
    inherited_base: str | None,
    sources: tuple[ReferenceSource, ...],
) -> DiscardedBodyResource:
    """Return one resource audit record with actual scoped reference edges.

    Returns:
        The resource metadata and every HTML path that references it.

    """
    content_id = _normalize_content_id_header(part.get(CONTENT_ID_HEADER))
    variants = _resource_location_variants(part, inherited_base)
    referenced_by = tuple(
        source_path
        for source_path, references in sources
        if (content_id is not None and content_id in references.content_ids)
        or bool(variants & references.locations)
    )
    return DiscardedBodyResource(
        path=path,
        referenced_by=referenced_by,
        content_type=part.get_content_type(),
        filename=part.get_filename(),
        disposition=part.get_content_disposition(),
    )


def resource_leaves(
    part: EmailMessage,
    path: MimePath,
    scan: ResourceScan,
) -> tuple[DiscardedBodyResource, ...]:
    """Collect resource leaves without traversing opaque protected entities.

    Returns:
        Resource records in original MIME-path order.

    """
    if (
        part.get_content_maintype() == "message"
        or part.get_content_type() in PROTECTED_MIME_TYPES
    ):
        record = (
            _body_resource(
                part,
                path,
                scan.inherited_base,
                scan.reference_sources,
            ),
        )
        return record if scan.root_inclusion is RootInclusion.INCLUDE else ()
    payload = part.get_payload()
    if _is_email_message_list(payload):
        if not payload:
            record = (
                _body_resource(
                    part,
                    path,
                    scan.inherited_base,
                    scan.reference_sources,
                ),
            )
            return record if scan.root_inclusion is RootInclusion.INCLUDE else ()
        child_base = _content_location_base(part, scan.inherited_base)
        return tuple(
            resource
            for index, child in enumerate(payload)
            for resource in resource_leaves(
                child,
                (*path, index),
                ResourceScan(
                    reference_sources=scan.reference_sources,
                    inherited_base=child_base,
                    include_text=scan.include_text,
                    root_inclusion=RootInclusion.INCLUDE,
                ),
            )
        )
    if scan.root_inclusion is not RootInclusion.INCLUDE or (
        not scan.include_text and part.get_content_type() in {"text/html", "text/plain"}
    ):
        return ()
    return (
        _body_resource(
            part,
            path,
            scan.inherited_base,
            scan.reference_sources,
        ),
    )

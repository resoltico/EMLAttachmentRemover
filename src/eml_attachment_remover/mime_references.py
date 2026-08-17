"""Extract body-resource references from MIME messages."""

from __future__ import annotations

import sys
from email.message import EmailMessage
from string import hexdigits
from typing import Final, TypeGuard
from urllib.parse import unquote, urljoin

from .css_references import _css_reference_values
from .html_references import _reference_values
from .mime_body import ATTACHMENT_DISPOSITION, INLINE_DISPOSITION
from .mime_locations import (
    _content_location_base,
    _html_mime_base,
    _location_variants,
    _normalize_percent_encoding,
)
from .models import ReferenceIndex
from .srcset_references import _srcset_urls

CONTENT_ID_HEADER: Final = "Content-ID"
CID_SCHEME: Final = "cid:"
DATA_SCHEME: Final = "data:"
CSS_WHITESPACE: Final = " \t\r\n\f"
MAX_CSS_ESCAPE_DIGITS: Final = 6
MAX_UNICODE_CODE_POINT: Final = 0x10FFFF
MIN_SURROGATE_CODE_POINT: Final = 0xD800
MAX_SURROGATE_CODE_POINT: Final = 0xDFFF


def _is_email_message_list(value: object) -> TypeGuard[list[EmailMessage]]:
    """Return whether a multipart payload contains traversable email entities.

    Returns:
        ``True`` only when every list member is an ``EmailMessage``.

    """
    return isinstance(value, list) and all(
        isinstance(child, EmailMessage) for child in value
    )


def _canonical_cid_uri(value: str) -> str | None:
    """Canonicalize one already-decoded CID URI.

    Returns:
        The exact identifier without CID or angle-bracket syntax, or ``None``.

    """
    normalized = value.strip()[len(CID_SCHEME) :].strip()
    if normalized.startswith("<") and normalized.endswith(">"):
        normalized = normalized[1:-1].strip()
    return normalized or None


def _normalize_content_id_header(value: object | None) -> str | None:
    """Normalize a raw Content-ID header without interpreting HTML entities.

    Returns:
        The exact header identifier without surrounding syntax, or ``None``.

    """
    if value is None:
        return None
    normalized = str(value).strip()
    if normalized.startswith("<") and normalized.endswith(">"):
        normalized = normalized[1:-1].strip()
    return normalized or None


def _consume_css_escape(value: str, index: int) -> tuple[str, int]:
    """Decode the CSS escape that begins immediately before ``index``.

    Returns:
        The decoded text and the index after the complete escape.

    """
    if index == len(value):
        return "\\", index
    if value[index] in "\r\n\f":
        width = 2 if value[index : index + 2] == "\r\n" else 1
        return "", index + width
    hexadecimal = value[index : index + MAX_CSS_ESCAPE_DIGITS]
    digit_count = 0
    for candidate in hexadecimal:
        if candidate not in hexdigits:
            break
        digit_count += 1
    end = index + digit_count
    if end == index:
        return value[index], index + 1
    code_point = int(value[index:end], 16)
    if (
        code_point == 0
        or code_point > MAX_UNICODE_CODE_POINT
        or MIN_SURROGATE_CODE_POINT <= code_point <= MAX_SURROGATE_CODE_POINT
    ):
        character = chr(0xFFFD)
    else:
        character = chr(code_point)
    if end < len(value) and value[end] in CSS_WHITESPACE:
        end += 1
    return character, end


def _decode_css_escapes(value: str) -> str:
    """Decode CSS escapes without rejecting malformed public input.

    Returns:
        Text with valid CSS escapes decoded and malformed trailing escapes retained.

    """
    decoded: list[str] = []
    index = 0
    for _iteration in range(len(value)):
        if index >= len(value):
            break
        character = value[index]
        if character != "\\":
            decoded.append(character)
            index += 1
        else:
            character, index = _consume_css_escape(value, index + 1)
            decoded.append(character)
    return "".join(decoded)


def _effective_html_base(html_base: str | None, mime_base: str | None) -> str | None:
    """Resolve the first HTML base element against MIME location metadata.

    Returns:
        The effective document base, or the MIME base when no HTML base exists.

    """
    if html_base is None:
        return mime_base
    html_base = _normalize_percent_encoding(html_base)
    if mime_base is None:
        return html_base
    try:
        return urljoin(mime_base, html_base)
    except ValueError:
        return mime_base


def _decode_text_payload(part: EmailMessage) -> str:
    """Decode one text payload without failing on a bad charset label.

    Returns:
        Decoded text, substituting replacement characters when necessary.

    """
    decoded = part.get_payload(decode=True)
    if not isinstance(decoded, bytes):
        return ""
    raw = decoded
    charset = part.get_content_charset() or sys.getdefaultencoding()
    try:
        return raw.decode(charset, errors="replace")
    except LookupError:
        return raw.decode(errors="replace")


def _record_uri_reference(
    represented_value: str,
    content_ids: set[str],
    locations: set[str],
    base: str | None,
) -> None:
    """Add one represented URI to the appropriate reference collection."""
    represented = represented_value.strip()
    if represented.casefold().startswith(CID_SCHEME):
        normalized_cid = _canonical_cid_uri(unquote(represented_value))
        if normalized_cid is not None:
            content_ids.add(normalized_cid)
        return
    if (
        not represented
        or represented.startswith("#")
        or represented.casefold().startswith(DATA_SCHEME)
    ):
        return
    locations.update(_location_variants(represented_value, base))


def _record_css_references(
    source: str,
    content_ids: set[str],
    locations: set[str],
    base: str | None,
) -> None:
    """Record actual CSS references using one known HTML decoding boundary."""
    for value in _css_reference_values(source):
        _record_uri_reference(
            _decode_css_escapes(value),
            content_ids,
            locations,
            base,
        )


def _references_from_html(
    source: str,
    *,
    base_location: str | None = None,
) -> ReferenceIndex:
    """Extract CID and location references from one decoded HTML body.

    Returns:
        Normalized CID and Content-Location references.

    """
    values = _reference_values(source)
    content_ids: set[str] = set()
    locations: set[str] = set()
    effective_base = _effective_html_base(values.base_href, base_location)

    for value in values.uris:
        _record_uri_reference(
            value,
            content_ids,
            locations,
            effective_base,
        )
    for value in values.srcsets:
        for candidate in _srcset_urls(value):
            _record_uri_reference(
                candidate,
                content_ids,
                locations,
                effective_base,
            )
    for css in values.css_attributes:
        _record_css_references(
            css,
            content_ids,
            locations,
            effective_base,
        )
    for css in values.style_elements:
        _record_css_references(
            css,
            content_ids,
            locations,
            effective_base,
        )
    return ReferenceIndex(frozenset(content_ids), frozenset(locations))


def _merge_references(left: ReferenceIndex, right: ReferenceIndex) -> ReferenceIndex:
    """Merge two immutable resource-reference indexes.

    Returns:
        One index containing the union of both inputs.

    """
    return ReferenceIndex(
        left.content_ids | right.content_ids,
        left.locations | right.locations,
    )


def _body_part_references(
    part: EmailMessage,
    parent_type: str | None,
    inherited_base: str | None,
    *,
    is_root: bool,
) -> ReferenceIndex:
    """Return references contributed by one eligible HTML body entity.

    Returns:
        Its references, or an empty index when the leaf is not body HTML.

    """
    if part.get_content_type() != "text/html":
        return ReferenceIndex(frozenset(), frozenset())
    disposition = part.get_content_disposition()
    if (
        not is_root
        and part.get_filename() is not None
        and disposition != INLINE_DISPOSITION
        and parent_type != "multipart/alternative"
    ):
        return ReferenceIndex(frozenset(), frozenset())
    return _references_from_html(
        _decode_text_payload(part),
        base_location=_html_mime_base(part, inherited_base),
    )


def _collect_references(
    message: EmailMessage,
    *,
    inherited_base: str | None = None,
) -> ReferenceIndex:
    """Collect body-resource references while skipping attachment subtrees.

    Returns:
        All normalized references found in HTML body content.

    """
    references = ReferenceIndex(frozenset(), frozenset())

    def visit(
        part: EmailMessage,
        *,
        inherited_base: str | None,
        ancestor_types: tuple[str, ...],
    ) -> None:
        nonlocal references
        is_root = part is message
        if not is_root and part.get_content_disposition() == ATTACHMENT_DISPOSITION:
            return
        if not is_root and part.get_content_maintype() == "message":
            return
        if not is_root and part.get_content_type() == "multipart/related":
            return
        location_base = _content_location_base(part, inherited_base)
        if not part.is_multipart():
            parent_type = ancestor_types[-1] if ancestor_types else None
            references = _merge_references(
                references,
                _body_part_references(
                    part,
                    parent_type,
                    inherited_base,
                    is_root=is_root,
                ),
            )
            return
        payload = part.get_payload()
        if not _is_email_message_list(payload):
            return
        content_type = part.get_content_type()
        for child in payload:
            visit(
                child,
                inherited_base=location_base,
                ancestor_types=(*ancestor_types, content_type),
            )

    visit(
        message,
        inherited_base=inherited_base,
        ancestor_types=(),
    )
    return references

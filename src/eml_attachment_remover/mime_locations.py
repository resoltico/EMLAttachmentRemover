"""Normalize and resolve MIME Content-Location resource labels."""

from __future__ import annotations

from contextlib import suppress
from string import ascii_letters, digits, hexdigits
from typing import TYPE_CHECKING, Final
from urllib.parse import SplitResult, urljoin, urlsplit, urlunsplit

if TYPE_CHECKING:
    from email.message import EmailMessage

CONTENT_LOCATION_HEADER: Final = "Content-Location"
CONTENT_BASE_HEADER: Final = "Content-Base"
UNRESERVED_URI_CHARACTERS: Final = frozenset(ascii_letters + digits + "-._~")
PERCENT_TRIPLET_LENGTH: Final = 3
DEFAULT_PORTS: Final = {"http": "80", "https": "443"}


def _normalize_percent_encoding(value: str) -> str:
    """Decode only unreserved URI octets and normalize other escape hex case.

    Returns:
        RFC 3986 percent-normalized text.

    """
    normalized: list[str] = []
    index = 0
    for _iteration in range(len(value)):
        if index >= len(value):
            break
        triplet = value[index : index + PERCENT_TRIPLET_LENGTH]
        if (
            len(triplet) == PERCENT_TRIPLET_LENGTH
            and triplet[0] == "%"
            and all(character in hexdigits for character in triplet[1:])
        ):
            character = chr(int(triplet[1:], 16))
            normalized.append(
                character
                if character in UNRESERVED_URI_CHARACTERS
                else f"%{triplet[1:].upper()}"
            )
            index += PERCENT_TRIPLET_LENGTH
            continue
        normalized.append(value[index])
        index += 1
    return "".join(normalized)


def _normalize_authority(parts: SplitResult) -> str:
    """Normalize URI scheme and hostname while preserving user information.

    Returns:
        A netloc with only its host name made case-insensitive.

    """
    if not parts.netloc:
        return parts.netloc
    user_info, marker, host_port = parts.netloc.rpartition("@")
    prefix = f"{user_info}@" if marker else ""
    if host_port.startswith("["):
        end = host_port.find("]")
        host = host_port[: end + 1].lower()
        suffix = host_port[end + 1 :]
        port = suffix.removeprefix(":") if suffix.startswith(":") else None
        suffix = "" if port == DEFAULT_PORTS.get(parts.scheme.lower()) else suffix
        return prefix + host + suffix
    host, separator, port = host_port.partition(":")
    default_port = DEFAULT_PORTS.get(parts.scheme.lower())
    suffix = f":{port}" if separator and port != default_port else ""
    return prefix + host.lower() + suffix


def _without_last_path_segment(value: str) -> str:
    """Remove the final path segment from an RFC 3986 output buffer.

    Returns:
        The path prefix before its final slash, or an empty string.

    """
    return value.rpartition("/")[0]


def _remove_dot_segments(path: str) -> str:
    """Apply the RFC 3986 dot-segment removal algorithm to one path.

    Returns:
        A path with literal and percent-normalized dot segments removed.

    """
    remaining = path
    output = ""
    for _iteration in range(len(path)):
        if not remaining:
            break
        remaining, output = _remove_dot_segment_step(remaining, output)
    return output


def _remove_dot_segment_step(remaining: str, output: str) -> tuple[str, str]:
    """Apply one RFC 3986 dot-segment transition.

    Returns:
        The remaining input and accumulated output after one transition.

    """
    if remaining.startswith(("../", "./")):
        remaining = remaining.partition("/")[2]
    elif remaining.startswith("/./"):
        remaining = remaining[2:]
    elif remaining == "/.":
        remaining = "/"
    elif remaining.startswith("/../"):
        remaining = remaining[3:]
        output = _without_last_path_segment(output)
    elif remaining == "/..":
        remaining = "/"
        output = _without_last_path_segment(output)
    elif remaining in {".", ".."}:
        return "", output
    else:
        if remaining.startswith("/"):
            prefix = "/"
            body = remaining[1:]
        else:
            prefix = ""
            body = remaining
        head, separator, tail = body.partition("/")
        output += prefix + head
        remaining = f"/{tail}" if separator else ""
    return remaining, output


def _normalize_path(path: str) -> str:
    """Normalize safe dot segments without losing unresolved parent traversal.

    Returns:
        An absolute normalized path, or a relative path retaining leading parents.

    """
    if path.startswith("/"):
        return _remove_dot_segments(path)
    segments: list[str] = []
    input_segments = path.split("/")
    for position, segment in enumerate(input_segments):
        if segment == ".":
            continue
        if segment != "..":
            segments.append(segment)
            continue
        if segments and segments[-1] not in {"", ".."}:
            segments.pop()
            if position == len(input_segments) - 1:
                segments.append("")
        else:
            segments.append(segment)
    normalized = "/".join(segments)
    trailing_directory = path.endswith("/.")
    return f"{normalized}/" if trailing_directory and normalized else normalized


def _restore_empty_delimiters(
    original: str,
    rebuilt: str,
    parts: SplitResult,
) -> str:
    """Restore empty query and fragment delimiters omitted by ``urlunsplit``.

    Returns:
        A normalized URI preserving syntactically present empty components.

    """
    before_fragment, fragment_marker, _fragment = original.partition("#")
    has_query_marker = "?" in before_fragment
    if has_query_marker and not parts.query:
        insertion = rebuilt.find("#")
        insertion = len(rebuilt) if insertion < 0 else insertion
        rebuilt = f"{rebuilt[:insertion]}?{rebuilt[insertion:]}"
    if fragment_marker and not parts.fragment:
        rebuilt += "#"
    return rebuilt


def _canonical_location(value: str) -> str | None:
    """Canonicalize one already-decoded Content-Location value.

    Returns:
        A percent-normalized location with only scheme and host folded, or ``None``.

    """
    normalized = _normalize_percent_encoding(value.strip())
    if not normalized:
        return None
    try:
        parts = urlsplit(normalized)
    except ValueError:
        return normalized
    path = _normalize_path(parts.path)
    rebuilt = urlunsplit((
        parts.scheme.lower(),
        _normalize_authority(parts),
        path,
        parts.query,
        parts.fragment,
    ))
    return _restore_empty_delimiters(normalized, rebuilt, parts)


def _normalize_content_location_header(value: object | None) -> str | None:
    """Normalize a raw Content-Location header without decoding HTML entities.

    Returns:
        A URI-syntax-normalized location without HTML decoding, or ``None``.

    """
    if value is None:
        return None
    return _canonical_location(str(value))


def _location_variants(
    value: str,
    base: str | None,
) -> frozenset[str]:
    """Return the effective normalized form of one location.

    Returns:
        Resolved variants when a base is usable, otherwise the raw relative label.

    """
    represented = _represented_location(value, base)
    variants: set[str | None] = set()
    variants.add(_canonical_location(represented))
    without_fragment, _marker, _fragment = represented.partition("#")
    variants.add(_canonical_location(without_fragment))
    return frozenset(variant for variant in variants if variant is not None)


def _represented_location(value: str, base: str | None) -> str:
    """Return a normalized location resolved against a usable base.

    Returns:
        The resolved representation, or the normalized relative value.

    """
    normalized = _normalize_percent_encoding(value)
    if base is None:
        return normalized
    with suppress(ValueError):
        return urljoin(base, normalized)
    return normalized


def _is_absolute_uri(value: str) -> bool:
    """Return whether a URI has an RFC 3986 scheme prefix.

    Returns:
        ``True`` for an absolute URI, otherwise ``False``.

    """
    scheme, separator, _remainder = value.partition(":")
    if not separator or not scheme or not scheme[0].isalpha():
        return False
    return all(
        character.isalnum() or character in {"+", "-", "."} for character in scheme
    )


def _content_location_base(part: EmailMessage, inherited: str | None) -> str | None:
    """Return the compatible base inherited by one MIME subtree.

    Returns:
        The nearest absolute Content-Location, or the inherited base.

    """
    content_base = part.get(CONTENT_BASE_HEADER)
    content_location = part.get(CONTENT_LOCATION_HEADER)
    location_header = content_base if content_base is not None else content_location
    if location_header is None:
        return inherited
    location = str(location_header)
    if inherited is not None:
        with suppress(ValueError):
            location = urljoin(inherited, location)
    return location if _is_absolute_uri(location) else inherited


def _html_mime_base(part: EmailMessage, inherited: str | None) -> str | None:
    """Return the effective MIME base for one HTML body part.

    Returns:
        Its legacy Content-Base, Content-Location, or inherited absolute base.

    """
    content_base = part.get(CONTENT_BASE_HEADER)
    if content_base is not None:
        base = str(content_base)
        if inherited is not None:
            with suppress(ValueError):
                base = urljoin(inherited, base)
        return base
    content_location = part.get(CONTENT_LOCATION_HEADER)
    if content_location is None:
        return inherited
    represented_location = str(content_location)
    if inherited is None:
        return represented_location
    with suppress(ValueError):
        return urljoin(inherited, represented_location)
    return inherited


def _resource_location_variants(
    part: EmailMessage,
    inherited_base: str | None,
) -> frozenset[str]:
    """Return direct and RFC 2557-resolved labels for one resource.

    Returns:
        Normalized variants of the part's Content-Location value.

    """
    content_location = part.get(CONTENT_LOCATION_HEADER)
    if content_location is None:
        return frozenset()
    raw_location = str(content_location)
    content_base = part.get(CONTENT_BASE_HEADER)
    resolution_base = str(content_base) if content_base is not None else inherited_base
    if content_base is not None and inherited_base is not None:
        with suppress(ValueError):
            resolution_base = urljoin(inherited_base, resolution_base)
    represented = _represented_location(raw_location, resolution_base)
    canonical = _canonical_location(represented)
    return frozenset() if canonical is None else frozenset({canonical})

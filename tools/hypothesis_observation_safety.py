"""Bound untrusted Hypothesis observation values for public retention."""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path
from typing import Final

PRIVATE_METADATA_FIELDS: Final = frozenset({
    "imported_at",
    "os.getpid",
    "os.getpid()",
    "sys.argv",
})
PROJECT_PLACEHOLDER: Final = "<project-root>"
HOME_PLACEHOLDER: Final = "<user-home>"
PYTHON_PLACEHOLDER: Final = "<python-prefix>"
PYTHON_BASE_PLACEHOLDER: Final = "<python-base-prefix>"
ABSOLUTE_POSIX_PATH: Final = re.compile(r"(?<![\w./:><*])/(?![/*])[^\s'\"<>,;:]+")
ABSOLUTE_WINDOWS_PATH: Final = re.compile(
    r"(?i)(?<![\w.])(?:[a-z]:[\\/]|\\\\)[^\s'\"<>,;:]+"
)
FILE_URI_PATH: Final = re.compile(r"(?i)file:(?://)?(?:/|[a-z]:[\\/]|\\\\)")
PUBLIC_WEB_URL: Final = re.compile(r"(?i)https?://[^\s'\"<>]+")
SAFE_FIELD_NAME: Final = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")
WINDOWS_PREFIX: Final = re.compile(r"(?i)^(?:[a-z]:[\\/]|\\\\)")
REDACTED_ARGUMENT: Final = "<redacted>"
REDACTED_REPRESENTATION: Final = "<redacted; structured argument names retained>"
RAW_OBSERVATION_FIELDS: Final = frozenset({"arguments", "representation"})

type JsonScalar = bool | float | int | str | None
type JsonValue = JsonScalar | list[JsonValue] | dict[str, JsonValue]


class HypothesisArtifactError(ValueError):
    """Report unsafe, corrupt, or unpublishable Hypothesis artifacts."""


def path_prefix_variants(prefix: str) -> tuple[str, ...]:
    """Return native, slash-normalized, and escaped path-prefix spellings.

    Returns:
        Deterministically ordered spellings found in raw and represented text.

    """
    variants = {prefix, prefix.replace("\\", "/")}
    if "\\" in prefix:
        variants.add(prefix.replace("\\", "\\\\"))
    return tuple(sorted(variants))


def replacement_prefixes(
    root: Path,
    additional: tuple[tuple[Path, str], ...] = (),
) -> tuple[tuple[str, str], ...]:
    """Return longest-first private prefixes and their public placeholders.

    Returns:
        Distinct non-root prefixes in deterministic replacement order.

    """
    candidates = (
        (str(root.resolve()), PROJECT_PLACEHOLDER),
        *((str(path.resolve()), replacement) for path, replacement in additional),
        (str(Path.home().resolve()), HOME_PLACEHOLDER),
        (str(Path(sys.prefix).resolve()), PYTHON_PLACEHOLDER),
        (str(Path(sys.base_prefix).resolve()), PYTHON_BASE_PLACEHOLDER),
    )
    replacements: dict[str, str] = {}
    for prefix, replacement in candidates:
        if prefix != os.sep:
            for variant in path_prefix_variants(prefix):
                replacements.setdefault(variant, replacement)
    return tuple(sorted(replacements.items(), key=lambda item: (-len(item[0]), item)))


def _path_prefix(prefix: str) -> bool:
    """Return whether a replacement prefix denotes an absolute path.

    Returns:
        Whether the prefix is an absolute POSIX, drive-letter, or UNC path.

    """
    return prefix.startswith("/") or WINDOWS_PREFIX.match(prefix) is not None


def _replace_prefix(value: str, prefix: str, replacement: str) -> str:
    """Replace one prefix, case-insensitively for Windows path spellings.

    Returns:
        Text with every matching prefix replaced.

    """
    flags = re.IGNORECASE if WINDOWS_PREFIX.match(prefix) is not None else 0
    return re.sub(
        re.escape(prefix),
        lambda _match: replacement,
        value,
        flags=flags,
    )


def public_text(value: str, replacements: tuple[tuple[str, str], ...]) -> str:
    """Return text with known private path prefixes replaced.

    Returns:
        Portable public observation text with forward-slash placeholder paths.

    """
    for prefix, replacement in replacements:
        value = _replace_prefix(value, prefix, replacement)
    path_placeholders = frozenset(
        replacement for prefix, replacement in replacements if _path_prefix(prefix)
    )
    lines: list[str] = []
    for raw_line in value.splitlines(keepends=True):
        public_line = raw_line
        if any(placeholder in public_line for placeholder in path_placeholders):
            while "\\\\" in public_line:
                public_line = public_line.replace("\\\\", "\\")
            public_line = public_line.replace("\\", "/")
        lines.append(public_line)
    return "".join(lines)


def private_prefix_remains(
    value: str,
    replacements: tuple[tuple[str, str], ...],
) -> bool:
    """Return whether any known private prefix remains after sanitization.

    Returns:
        Whether sanitized public text still contains a private prefix.

    """
    return any(
        re.search(
            re.escape(prefix),
            value,
            flags=re.IGNORECASE if WINDOWS_PREFIX.match(prefix) is not None else 0,
        )
        is not None
        for prefix, _replacement in replacements
    )


def contains_absolute_path(value: str) -> bool:
    """Return whether text retains a POSIX, drive-letter, or UNC path.

    Returns:
        Whether the text contains a residual absolute path.

    """
    if FILE_URI_PATH.search(value):
        return True
    return any(
        ABSOLUTE_POSIX_PATH.search(segment) or ABSOLUTE_WINDOWS_PATH.search(segment)
        for segment in PUBLIC_WEB_URL.split(value)
    )


def reject_absolute_paths(
    value: JsonValue,
    path: Path,
    line_number: int,
    field: str = "$",
) -> None:
    """Reject residual absolute paths anywhere in retained public JSON.

    Raises:
        HypothesisArtifactError: If a key or value retains an absolute path.

    """
    if isinstance(value, str) and contains_absolute_path(value):
        message = (
            "machine-specific absolute path remains in Hypothesis observation "
            f"{path.name!r}:{line_number} at {field}; value redacted"
        )
        raise HypothesisArtifactError(message)
    if isinstance(value, list):
        for index, item in enumerate(value):
            reject_absolute_paths(item, path, line_number, f"{field}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            reject_absolute_paths(key, path, line_number, f"{field}.<object-key>")
            child = f"{field}.{key}" if SAFE_FIELD_NAME.fullmatch(key) else field
            reject_absolute_paths(item, path, line_number, child)


def public_value(
    value: JsonValue,
    replacements: tuple[tuple[str, str], ...],
    *,
    observation_root: bool = False,
) -> JsonValue:
    """Return a recursively sanitized JSON-compatible value.

    Returns:
        The public value with only top-level schema fields specially bounded.

    """
    if isinstance(value, str):
        return public_text(value, replacements)
    if isinstance(value, list):
        return [public_value(item, replacements) for item in value]
    if not isinstance(value, dict):
        return value
    public: dict[str, JsonValue] = {}
    for key, item in value.items():
        if observation_root:
            handled, root_value = _public_root_field(key, item, replacements)
            if handled:
                public[key] = root_value
                continue
        public[public_text(key, replacements)] = public_value(item, replacements)
    return public


def _public_root_field(
    key: str,
    value: JsonValue,
    replacements: tuple[tuple[str, str], ...],
) -> tuple[bool, JsonValue]:
    """Return whether and how one top-level observation field is bounded.

    Returns:
        A handled flag and the bounded field value.

    """
    if key == "coverage":
        return True, None
    if key in RAW_OBSERVATION_FIELDS:
        return True, _redacted_raw_field(key, value, replacements)
    if key == "metadata" and isinstance(value, dict):
        return True, {
            name: public_value(item, replacements)
            for name, item in value.items()
            if name not in PRIVATE_METADATA_FIELDS
        }
    return False, value


def _redacted_raw_field(
    key: str,
    value: JsonValue,
    replacements: tuple[tuple[str, str], ...],
) -> JsonValue:
    """Return a bounded replacement for an unbounded generated value.

    Returns:
        A representation sentinel or redacted argument-name mapping.

    """
    if key == "representation":
        return REDACTED_REPRESENTATION
    if isinstance(value, dict):
        return {public_text(name, replacements): REDACTED_ARGUMENT for name in value}
    return REDACTED_ARGUMENT

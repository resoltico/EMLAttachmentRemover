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
REDACTED_ARGUMENT: Final = "<redacted>"
REDACTED_REPRESENTATION: Final = "<redacted; structured argument names retained>"
RAW_OBSERVATION_FIELDS: Final = frozenset({"arguments", "representation"})

type JsonScalar = bool | float | int | str | None
type JsonValue = JsonScalar | list[JsonValue] | dict[str, JsonValue]


class HypothesisArtifactError(ValueError):
    """Report unsafe, corrupt, or unpublishable Hypothesis artifacts."""


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
            replacements.setdefault(prefix, replacement)
            replacements.setdefault(prefix.replace("\\", "/"), replacement)
    return tuple(sorted(replacements.items(), key=lambda item: (-len(item[0]), item)))


def _public_text(value: str, replacements: tuple[tuple[str, str], ...]) -> str:
    """Return text with known private path prefixes replaced.

    Returns:
        Portable public observation text.

    """
    for prefix, replacement in replacements:
        value = value.replace(prefix, replacement)
    return value


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
        return _public_text(value, replacements)
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
        public[_public_text(key, replacements)] = public_value(item, replacements)
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
        return {_public_text(name, replacements): REDACTED_ARGUMENT for name in value}
    return REDACTED_ARGUMENT

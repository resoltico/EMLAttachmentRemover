"""Validate source-bound reviewed-equivalent mutation manifests."""

from __future__ import annotations

import hashlib
import json
import re
from json import JSONDecodeError
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Sequence

RESULT_PATTERN: Final = re.compile(
    r"^(?P<mutant>[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*): "
    r"(?P<status>[a-z][a-z ]*[a-z])$",
)
EQUIVALENT_FIELDS: Final = frozenset({"mutant", "rationale"})
MANIFEST_FIELDS: Final = frozenset({
    "schema_version",
    "source_sha256",
    "equivalents",
})
SHA256_PATTERN: Final = re.compile(r"[0-9a-f]{8}(?::[0-9a-f]{8}){7}")
UTF8: Final = "utf-8"
BOUNDARY_BYTES: Final = 8
DIGEST_WORD_CHARACTERS: Final = 8
MANIFEST_LABEL: Final = "equivalent-mutant manifest"
MANIFEST_SCHEMA_MESSAGE: Final = (
    "equivalent-mutant manifest schema_version must be integer 1"
)
MANIFEST_ARRAY_MESSAGE: Final = (
    "equivalent-mutant manifest equivalents must be a JSON array"
)
MANIFEST_ORDER_MESSAGE: Final = (
    "equivalent-mutant entries must be in lexical mutant-ID order"
)
EMPTY_ROOTS_MESSAGE: Final = "production source roots must not be empty"


class MutationResultsError(ValueError):
    """Report unusable mutation evidence or an unsuccessful mutation run."""


def read_text(path: Path, label: str) -> str:
    """Read one evidence file or raise a contextual validation error.

    Returns:
        The UTF-8 file contents.

    Raises:
        MutationResultsError: If the file cannot be read.

    """
    try:
        return path.read_text(encoding=UTF8)
    except OSError as error:
        message = f"cannot read {label} {path}: {error}"
        raise MutationResultsError(message) from error


def load_object(path: Path, label: str) -> dict[str, object]:
    """Load a strict JSON object from one evidence file.

    Returns:
        A string-keyed JSON object.

    Raises:
        MutationResultsError: If the JSON syntax or outer type is invalid.

    """
    try:
        loaded: object = json.loads(read_text(path, label))
    except JSONDecodeError as error:
        message = f"{label} is not valid JSON: {error}"
        raise MutationResultsError(message) from error
    if not isinstance(loaded, dict):
        message = f"{label} must be a JSON object"
        raise MutationResultsError(message)
    if not all(type(key) is str for key in loaded):
        message = f"{label} field names must be strings"
        raise MutationResultsError(message)
    return loaded


def require_fields(
    values: dict[str, object],
    expected: frozenset[str],
    label: str,
) -> None:
    """Require one JSON object to contain exactly the expected field names.

    Raises:
        MutationResultsError: If any field is missing or unknown.

    """
    actual = frozenset(values)
    if actual != expected:
        missing = sorted(expected - actual)
        unknown = sorted(actual - expected)
        message = f"{label} schema mismatch: missing={missing}; unknown={unknown}"
        raise MutationResultsError(message)


def source_sha256(source_roots: Path | Sequence[Path]) -> str:
    """Hash sorted production Python roots and bytes with explicit boundaries.

    Returns:
        A deterministic lower-case SHA-256 digest for all production source roots.

    Raises:
        MutationResultsError: If the tree is empty, unsafe, or unreadable.

    """
    roots = (source_roots,) if isinstance(source_roots, Path) else tuple(source_roots)
    if not roots:
        raise MutationResultsError(EMPTY_ROOTS_MESSAGE)
    digest = hashlib.sha256()
    for index, source_root in enumerate(roots):
        if source_root.is_symlink() or not source_root.is_dir():
            message = f"production source root must be a real directory: {source_root}"
            raise MutationResultsError(message)
        paths_by_name = {
            path.relative_to(source_root).as_posix(): path
            for path in source_root.rglob("*.py")
        }
        if not paths_by_name:
            message = f"production source root contains no Python files: {source_root}"
            raise MutationResultsError(message)
        digest.update(index.to_bytes(BOUNDARY_BYTES))
        for relative_name in sorted(paths_by_name):
            path = paths_by_name[relative_name]
            if path.is_symlink() or not path.is_file():
                message = (
                    f"production source must be a regular non-symbolic file: {path}"
                )
                raise MutationResultsError(message)
            relative = relative_name.encode()
            try:
                content = path.read_bytes()
            except OSError as error:
                message = f"cannot read production source {path}: {error}"
                raise MutationResultsError(message) from error
            digest.update(len(relative).to_bytes(BOUNDARY_BYTES))
            digest.update(relative)
            digest.update(len(content).to_bytes(BOUNDARY_BYTES))
            digest.update(content)
    hexadecimal = digest.hexdigest()
    return ":".join(
        hexadecimal[index : index + DIGEST_WORD_CHARACTERS]
        for index in range(0, len(hexadecimal), DIGEST_WORD_CHARACTERS)
    )


def validate_equivalent_entry(entry_value: object, index: int) -> tuple[str, str]:
    """Validate one exact mutant ID and its public equivalence rationale.

    Returns:
        A normalized mutant ID and rationale.

    Raises:
        MutationResultsError: If the entry is malformed.

    """
    if not isinstance(entry_value, dict) or not all(
        type(key) is str for key in entry_value
    ):
        message = f"equivalent-mutant entry {index} must be a JSON object"
        raise MutationResultsError(message)
    entry = entry_value
    require_fields(entry, EQUIVALENT_FIELDS, f"equivalent-mutant entry {index}")
    mutant = entry["mutant"]
    rationale = entry["rationale"]
    if (
        type(mutant) is not str
        or RESULT_PATTERN.fullmatch(f"{mutant}: survived") is None
    ):
        message = f"equivalent-mutant entry {index} has an invalid mutant ID"
        raise MutationResultsError(message)
    if type(rationale) is not str or not rationale.strip():
        message = f"equivalent-mutant entry {index} requires a nonempty rationale"
        raise MutationResultsError(message)
    if rationale != rationale.strip():
        message = f"equivalent-mutant entry {index} rationale must be normalized"
        raise MutationResultsError(message)
    return mutant, rationale


def load_equivalents(path: Path, source_roots: Path | Sequence[Path]) -> dict[str, str]:
    """Load the exact reviewed-equivalent mutant manifest.

    Returns:
        Exact mutant IDs mapped to nonempty review rationales.

    Raises:
        MutationResultsError: If the manifest is malformed or ambiguous.

    """
    values = load_object(path, MANIFEST_LABEL)
    require_fields(values, MANIFEST_FIELDS, MANIFEST_LABEL)
    if type(values["schema_version"]) is not int or values["schema_version"] != 1:
        raise MutationResultsError(MANIFEST_SCHEMA_MESSAGE)
    source_digest = values["source_sha256"]
    if (
        type(source_digest) is not str
        or SHA256_PATTERN.fullmatch(source_digest) is None
    ):
        message = (
            "equivalent-mutant manifest source_sha256 must be eight colon-delimited "
            "lowercase hexadecimal words"
        )
        raise MutationResultsError(message)
    actual_digest = source_sha256(source_roots)
    if source_digest != actual_digest:
        message = (
            "equivalent-mutant manifest source_sha256 does not match production "
            f"source: expected={source_digest}, actual={actual_digest}; re-review "
            "every equivalent mutant after source changes"
        )
        raise MutationResultsError(message)
    entries = values["equivalents"]
    if not isinstance(entries, list):
        raise MutationResultsError(MANIFEST_ARRAY_MESSAGE)
    equivalents: dict[str, str] = {}
    for index, untyped_entry in enumerate(entries):
        mutant, rationale = validate_equivalent_entry(untyped_entry, index)
        if mutant in equivalents:
            message = f"duplicate equivalent-mutant ID: {mutant}"
            raise MutationResultsError(message)
        equivalents[mutant] = rationale
    if list(equivalents) != sorted(equivalents):
        raise MutationResultsError(MANIFEST_ORDER_MESSAGE)
    return equivalents

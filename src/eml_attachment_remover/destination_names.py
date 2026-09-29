"""Fit an automatically derived output name to its directory's filename limit."""

from __future__ import annotations

import hashlib
import ntpath
import os
import posixpath
from pathlib import Path
from typing import Final

from .native_values import default_destination

SUFFIX: Final = ".mime-pruned.eml"
NAME_LIMIT_FALLBACK: Final = 255
DIGEST_CHARACTERS: Final = 16
UTF16_UNIT_BYTES: Final = 2


def native_bytes(text: str) -> bytes:
    """Return a name's exact native form: POSIX bytes or UTF-16 code units.

    Returns:
        The bytes the platform stores for the name.

    """
    if os.name == "nt":
        return text.encode("utf-16-le", "surrogatepass")
    return os.fsencode(text)


def native_length(text: str) -> int:
    """Measure a name in the backend's own unit: bytes, or UTF-16 code units.

    Returns:
        The name's length against a directory's component limit.

    """
    encoded = native_bytes(text)
    return len(encoded) // UTF16_UNIT_BYTES if os.name == "nt" else len(encoded)


def name_limit(directory: str) -> int:
    """Return the longest filename component the destination directory accepts.

    POSIX asks the kernel about the directory expression exactly as the later open
    will traverse it; Windows filesystems share a 255-unit component limit. An
    unanswerable question keeps the conservative common limit.

    Returns:
        The limit in the backend's own unit.

    """
    pathconf = getattr(os, "pathconf", None)
    if os.name == "nt" or pathconf is None:
        return NAME_LIMIT_FALLBACK
    try:
        limit = int(pathconf(directory or ".", "PC_NAME_MAX"))
    except OSError, ValueError:
        return NAME_LIMIT_FALLBACK
    return limit if limit > 0 else NAME_LIMIT_FALLBACK


def _prefix(text: str, budget: int) -> str:
    """Keep the longest whole-character prefix that fits ``budget`` native units.

    Returns:
        The prefix; never splits a character, so it stays valid text.

    """
    used = 0
    for count, character in enumerate(text):
        used += native_length(character)
        if used > budget:
            return text[:count]
    return text


def fit_name(source_name: str, derived: str, limit: int) -> str:
    """Return the derived name, shortened with a stable hash only if it cannot fit.

    The digest covers the complete native source name, so the result is the same on
    every run and two different long names never share an output.

    Returns:
        ``derived`` when it fits, else ``<prefix>-<digest>.mime-pruned.eml``.

    """
    if native_length(derived) <= limit:
        return derived
    digest = hashlib.sha256(native_bytes(source_name)).hexdigest()[:DIGEST_CHARACTERS]
    tail = f"-{digest}{SUFFIX}"
    # A name only needs shortening when even its stem outgrows the kept prefix, so
    # the prefix never reaches the source's own extension: no need to strip it.
    return _prefix(source_name, limit - native_length(tail)) + tail


def fitted_default_destination(source: str, output_dir: str | None) -> str:
    """Derive the default destination, shortening only its automatic name.

    The parent expression is preserved exactly; an explicit ``--output`` never
    reaches this function and is never renamed.

    Returns:
        The destination path intent before native binding.

    """
    default = default_destination(source)
    split = ntpath.split if os.name == "nt" else posixpath.split
    parent, derived = split(default)
    fitted = fit_name(
        split(source)[1],
        derived,
        name_limit(parent if output_dir is None else output_dir),
    )
    if output_dir is not None:
        return os.fspath(Path(output_dir) / fitted)
    return default.removesuffix(derived) + fitted

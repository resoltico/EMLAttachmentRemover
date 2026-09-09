"""Redact pytest node IDs whose escaped values contain an absolute path."""

from __future__ import annotations

import hashlib
import importlib
from typing import Final

path_safety = importlib.import_module(
    "tools.hypothesis_observation_safety"
    if __package__
    else "hypothesis_observation_safety"
)

UTF8: Final = "utf-8"
REDACTED_PREFIX: Final = "redacted-absolute-node-id-"
DIGEST_LENGTH: Final = 16


def redact(value: str) -> str:
    """Return a stable non-path token for an otherwise unsafe node ID.

    Returns:
        The original node ID unless its unescaped spelling is absolute.

    """
    if not path_safety.contains_absolute_path(value.replace("\\\\", "\\")):
        return value
    digest = hashlib.sha256(value.encode(UTF8, "backslashreplace")).hexdigest()
    return f"{REDACTED_PREFIX}{digest[:DIGEST_LENGTH]}"

"""Share the actual build invocation time across archive builders."""

from __future__ import annotations

import os
import struct
import time
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from pathlib import Path

# ZIP stores local wall time in whole two-second intervals.
EPOCH: Final = int(os.environ.get("SOURCE_DATE_EPOCH", str(int(time.time())))) // 2 * 2
ZIP_TIME: Final = time.localtime(EPOCH)[:6]
ZIP_FIELD_SIZE: Final = 25


def environment() -> dict[str, str]:
    """Return the build time for child builders.

    Returns:
        The standard build timestamp environment setting.

    """
    return {"SOURCE_DATE_EPOCH": str(EPOCH)}


def zip_extra(timestamp: int = EPOCH) -> bytes:
    """Encode the standard UTC modification time used by ZIP extractors.

    Returns:
        A Unix extended timestamp field containing the modification time.

    """
    # macOS ditto honors the Info-ZIP Unix field, not the newer UT field.
    return struct.pack("<HHBI", 0x5455, 5, 1, timestamp) + struct.pack(
        "<HHIIHH", 0x5855, 12, timestamp, timestamp, 0, 0
    )


def stamp_tree(root: Path) -> None:
    """Apply build time after creating all children, including directories."""
    for path in (*root.rglob("*"), root):
        os.utime(path, (EPOCH, EPOCH))


def zip_epoch(extra: bytes) -> int:
    """Read our exact Unix timestamp field, rejecting unrelated ZIP metadata.

    Returns:
        The absolute build time encoded in the ZIP member.

    Raises:
        ValueError: If the member lacks the exact timestamp field.

    """
    if len(extra) != ZIP_FIELD_SIZE or extra[:5] != b"\x55\x54\x05\x00\x01":
        message = "ZIP build timestamp field is invalid"
        raise ValueError(message)
    timestamp = int(struct.unpack("<I", extra[5:9])[0])
    if extra != zip_extra(timestamp):
        message = "ZIP build timestamp field is invalid"
        raise ValueError(message)
    return timestamp

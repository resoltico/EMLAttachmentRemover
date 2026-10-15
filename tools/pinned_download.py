"""Download tool archives without exceeding their independently pinned size."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from pathlib import Path
    from typing import BinaryIO

CHUNK: Final = 1024 * 1024


def copy_archive(source: BinaryIO, target: Path, *, size: int, sha256: str) -> None:
    """Write at most the pinned byte count and authenticate the complete archive.

    The caller owns a temporary directory, including cleanup on transfer failure.

    Raises:
        ValueError: If the received size or digest differs from the pin.

    """
    digest = hashlib.sha256()
    received = 0
    with target.open("xb") as stream:
        while block := source.read(CHUNK):
            received += len(block)
            if received > size:
                message = "tool download exceeds pinned byte count"
                raise ValueError(message)
            stream.write(block)
            digest.update(block)
    if received != size or digest.hexdigest() != sha256:
        message = "tool download size or checksum differs from pin"
        raise ValueError(message)

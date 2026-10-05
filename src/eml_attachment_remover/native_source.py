"""Read bounded source bytes through cancellable native descriptors."""

from __future__ import annotations

import os

from .cancellation import checkpoint
from .domain import AppError, ExitCode
from .native_values import MAX_RAW_BYTES


def read_source_bytes(descriptor: int, *, max_bytes: int = MAX_RAW_BYTES) -> bytes:
    """Read a source while preserving the size bound and cancellation checkpoints.

    Returns:
        Complete source bytes read through bounded chunks.

    Raises:
        AppError: If the source grows beyond its raw-size limit.

    """
    chunks: list[bytes] = []
    size = 0
    while chunk := os.read(descriptor, min(1024 * 1024, max_bytes - size + 1)):
        checkpoint()
        size += len(chunk)
        if size > max_bytes:
            raise AppError(
                ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
            )
        chunks.append(chunk)
    return b"".join(chunks)

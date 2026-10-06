"""Bounded, native-path request framing for pipe-based frontends."""

from __future__ import annotations

import base64
import json
import os
import time
from typing import Final

from .batch import MAX_BATCH_ITEMS, MAX_CUMULATIVE_REQUEST_PATH_BYTES
from .cancellation import POLL_SECONDS, checkpoint
from .domain import AppError, ExitCode

MAX_FRAME_BYTES: Final = 3 * MAX_CUMULATIVE_REQUEST_PATH_BYTES
HEADER_BYTES: Final = 4


def _object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    """Construct one request object, refusing duplicate field names.

    Returns:
        The uniquely keyed JSON object.

    Raises:
        ValueError: If an object repeats a field.

    """
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            message = "duplicate request field"
            raise ValueError(message)
        result[key] = value
    return result


def decode(payload: bytes) -> list[str]:
    """Decode a complete bounded request without interpreting paths as options.

    Returns:
        Exact host-native source strings in request order.

    Raises:
        AppError: If framing, encoding, fields or request limits are invalid.

    """
    if not payload or len(payload) > MAX_FRAME_BYTES:
        raise AppError(ExitCode.USAGE, "request frame exceeds its byte limit")
    try:
        value: object = json.loads(payload.decode("utf-8"), object_pairs_hook=_object)
    except (ValueError, RecursionError) as error:
        raise AppError(ExitCode.USAGE, "request is not valid UTF-8 JSON") from error
    encoding = "windows-utf16le" if os.name == "nt" else "posix-bytes"
    if (
        not isinstance(value, dict)
        or set(value) != {"encoding", "paths"}
        or value["encoding"] != encoding
        or not isinstance(value["paths"], list)
    ):
        raise AppError(ExitCode.USAGE, "invalid native-path request fields")
    entries = value["paths"]
    if not 0 < len(entries) <= MAX_BATCH_ITEMS:
        raise AppError(ExitCode.USAGE, "request exceeds its input-count limit")
    paths = [_path(entry, encoding) for entry in entries]
    if (
        sum(len(os.fsencode(path)) for path in paths)
        > MAX_CUMULATIVE_REQUEST_PATH_BYTES
    ):
        raise AppError(ExitCode.USAGE, "request exceeds its path-byte limit")
    return paths


def _path(value: object, encoding: str) -> str:
    """Decode one explicitly native address.

    Returns:
        A nonempty path containing no NUL code unit.

    Raises:
        AppError: If the value cannot represent a host-native path.

    """
    if not isinstance(value, str):
        raise AppError(ExitCode.USAGE, "request path is not Base64 text")
    try:
        raw = base64.b64decode(value, validate=True)
        path = (
            raw.decode("utf-16-le", "surrogatepass")
            if encoding == "windows-utf16le"
            else os.fsdecode(raw)
        )
    except ValueError as error:
        raise AppError(ExitCode.USAGE, "invalid native path encoding") from error
    if not path or "\0" in path:
        raise AppError(ExitCode.USAGE, "request path is empty or contains NUL")
    return path


def _read_exact(descriptor: int, size: int) -> bytes:
    """Read exactly one bounded segment with cooperative interruption checkpoints.

    Returns:
        The requested bytes, without consuming the following lifetime channel.

    Raises:
        AppError: If input ends early or cannot be read.

    """
    result = bytearray()
    while len(result) < size:
        checkpoint()
        try:
            chunk = os.read(descriptor, size - len(result))
        except BlockingIOError:
            time.sleep(POLL_SECONDS)
            continue
        except OSError as error:
            raise AppError(ExitCode.USAGE, "could not read request pipe") from error
        if not chunk:
            raise AppError(ExitCode.USAGE, "request pipe ended before a complete frame")
        result.extend(chunk)
    return bytes(result)


def read(descriptor: int) -> list[str]:
    """Read a four-byte big-endian length and one UTF-8 JSON request.

    Returns:
        The validated source selection.

    Raises:
        AppError: If a header or payload is invalid or incomplete.

    """
    size = int.from_bytes(_read_exact(descriptor, HEADER_BYTES), "big")
    if not 0 < size <= MAX_FRAME_BYTES:
        raise AppError(ExitCode.USAGE, "request frame exceeds its byte limit")
    return decode(_read_exact(descriptor, size))

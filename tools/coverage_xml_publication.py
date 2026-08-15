"""Read and atomically publish strictly validated Coverage.py XML."""

from __future__ import annotations

import os
import stat
import tempfile
from contextlib import suppress
from pathlib import Path
from typing import Final

if __package__:
    from tools.coverage_xml_validation import CoverageXmlError, validate
else:
    from coverage_xml_validation import CoverageXmlError, validate  # type: ignore[import-not-found,no-redef]  # ruff: ignore[unsorted-imports]

TEMPORARY_SUFFIX: Final = ".tmp"


def _regular_source(path: Path) -> bytes:
    """Read one regular non-symbolic staged report.

    Returns:
        The exact staged bytes.

    Raises:
        CoverageXmlError: If the source cannot be safely inspected or read.

    """
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        message = "cannot inspect staged Coverage XML"
        raise CoverageXmlError(message) from error
    if not stat.S_ISREG(mode):
        message = "staged Coverage XML must be a regular file"
        raise CoverageXmlError(message)
    try:
        return path.read_bytes()
    except OSError as error:
        message = "cannot read staged Coverage XML"
        raise CoverageXmlError(message) from error


def load_validated(source: Path) -> bytes:
    """Read and validate staged Coverage XML before private-temp cleanup.

    Returns:
        The validated exact bytes.

    """
    content = _regular_source(source)
    validate(content)
    return content


def _validate_destination(destination: Path) -> None:
    """Require a real parent and an absent-or-regular destination.

    Raises:
        CoverageXmlError: If the destination cannot be replaced safely.

    """
    try:
        parent_mode = destination.parent.lstat().st_mode
    except OSError as error:
        message = "cannot inspect Coverage XML directory"
        raise CoverageXmlError(message) from error
    if not stat.S_ISDIR(parent_mode):
        message = "Coverage XML directory must be real"
        raise CoverageXmlError(message)
    try:
        destination_mode = destination.lstat().st_mode
    except FileNotFoundError:
        return
    except OSError as error:
        message = "cannot inspect Coverage XML destination"
        raise CoverageXmlError(message) from error
    if not stat.S_ISREG(destination_mode):
        message = "Coverage XML destination must be regular"
        raise CoverageXmlError(message)


def _write_complete(descriptor: int, content: bytes) -> None:
    """Write, flush, and sync all bytes to one reserved temporary file.

    Raises:
        OSError: If the file cannot be written completely and durably.

    """
    try:
        output = os.fdopen(descriptor, "wb")
    except BaseException:
        os.close(descriptor)
        raise
    with output:
        written = output.write(content)
        if written != len(content):
            message = "Coverage XML publication write was incomplete"
            raise OSError(message)
        output.flush()
        os.fsync(output.fileno())


def publish(content: bytes, destination: Path) -> Path:
    """Validate and atomically replace one public Coverage XML report.

    Returns:
        The exact published destination.

    Raises:
        CoverageXmlError: If validation or atomic publication fails.

    """
    validate(content)
    try:
        destination.parent.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        message = "cannot create Coverage XML directory"
        raise CoverageXmlError(message) from error
    _validate_destination(destination)
    try:
        descriptor, temporary_name = tempfile.mkstemp(
            dir=destination.parent,
            prefix=f".{destination.name}.",
            suffix=TEMPORARY_SUFFIX,
        )
        temporary = Path(temporary_name)
    except OSError as error:
        message = "cannot reserve Coverage XML publication"
        raise CoverageXmlError(message) from error
    try:
        _write_complete(descriptor, content)
        temporary.replace(destination)
    except OSError as error:
        message = "cannot publish Coverage XML atomically"
        raise CoverageXmlError(message) from error
    finally:
        with suppress(OSError):
            temporary.unlink()
    return destination

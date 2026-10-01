"""Private bounded storage for terminal schema-3 item records."""

from __future__ import annotations

import os
import tempfile
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from collections.abc import Generator
    from typing import BinaryIO

MAX_RECORD_BYTES: Final = 1024 * 1024
MAX_SPOOL_BYTES: Final = 64 * 1024 * 1024
PROJECT_ROOT: Final = Path(__file__).resolve().parents[2]
# Only a source checkout has project storage to protect. A zipapp or installed
# package sits beside unrelated user directories that are valid temporary roots.
SOURCE_CHECKOUT: Final = (PROJECT_ROOT / "pyproject.toml").is_file()


class ReportSpoolError(OSError):
    """Report an unsafe, incomplete, or capacity-exceeding terminal-record spool."""


def private_temp_root() -> Path:
    """Return a real OS-private temporary root that is outside this repository.

    Returns:
        A resolved real temporary directory outside the project root.

    Raises:
        ReportSpoolError: If the environment-selected temporary root is unsafe.

    """
    selected_root = Path(tempfile.gettempdir())
    if selected_root.is_symlink() or not selected_root.is_dir():
        message = "private terminal report root is unsafe"
        raise ReportSpoolError(message)
    root = selected_root.resolve()
    if SOURCE_CHECKOUT and root.is_relative_to(PROJECT_ROOT):
        message = "private terminal report root is unsafe"
        raise ReportSpoolError(message)
    return root


def _write_all(descriptor: int, payload: bytes) -> None:
    """Write one record fully, rejecting a non-progressing filesystem boundary.

    Raises:
        ReportSpoolError: If a write does not make bounded forward progress.

    """
    position = 0
    for _ in range(len(payload)):
        written = os.write(descriptor, payload[position:])
        if written <= 0 or written > len(payload) - position:
            message = "terminal report spool write made no progress"
            raise ReportSpoolError(message)
        position += written
        if position == len(payload):
            return


def _truncate_or_raise(descriptor: int, offset: int) -> None:
    """Restore an interrupted append to its exact prior byte length.

    Raises:
        ReportSpoolError: If the prior length cannot be restored.

    """
    try:
        os.ftruncate(descriptor, offset)
    except OSError as error:
        message = "terminal report spool could not recover a partial record"
        raise ReportSpoolError(message) from error


@dataclass(slots=True)
class ReportSpool:
    """Own an OS-lifetime temporary stream, never reopened through a pathname."""

    file: BinaryIO
    bytes_written: int = 0
    record_count: int = 0
    closed: bool = False

    def __del__(self) -> None:
        """Close a discarded owner; forced-exit deletion remains an OS guarantee."""
        owned = getattr(self, "file", None)
        if owned is not None:
            with suppress(OSError):
                owned.close()

    @classmethod
    def create(cls) -> ReportSpool:
        """Create one private external spool with a restrictive filesystem mode.

        Returns:
            The sole owner of a new empty report spool.

        """
        # POSIX storage is anonymous or unlinked at creation. On Windows the
        # stdlib uses a delete-on-close handle, including process termination.
        return cls(
            cast(
                "BinaryIO",
                tempfile.TemporaryFile(
                    mode="w+b",
                    prefix=".eml-attachment-remover-report-",
                    dir=private_temp_root(),
                ),
            )
        )

    def append(self, record: bytes) -> None:
        """Append one bounded complete JSON record or fail before accepting it.

        Raises:
            ReportSpoolError: If the record, spool state, or write result is unsafe.

        """
        if (
            self.closed
            or self.file.closed
            or not record
            or b"\n" in record
            or len(record) > MAX_RECORD_BYTES
        ):
            message = "terminal report record is unsafe"
            raise ReportSpoolError(message)
        payload = record + b"\n"
        if self.bytes_written + len(payload) > MAX_SPOOL_BYTES:
            message = "terminal report spool exceeds its bounded capacity"
            raise ReportSpoolError(message)
        self.file.seek(0, os.SEEK_END)
        descriptor = self.file.fileno()
        committed = False
        try:
            _write_all(descriptor, payload)
            committed = True
        except ReportSpoolError:
            raise
        except OSError as error:
            message = "terminal report spool write failed"
            raise ReportSpoolError(message) from error
        finally:
            if not committed:
                _truncate_or_raise(descriptor, self.bytes_written)
        self.bytes_written += len(payload)
        self.record_count += 1

    def records(self) -> Generator[bytes]:
        """Yield every complete bounded record after validating the spool shape.

        Yields:
            Each ordered complete JSON record without its newline delimiter.

        Raises:
            ReportSpoolError: If the owned spool is unavailable or malformed.

        """
        if self.closed or self.file.closed:
            message = "terminal report spool is unavailable"
            raise ReportSpoolError(message)
        count = 0
        total = 0
        self.file.seek(0)
        while line := self.file.readline(MAX_RECORD_BYTES + 2):
            if not line.endswith(b"\n") or len(line) > MAX_RECORD_BYTES + 1:
                message = "terminal report spool is corrupt"
                raise ReportSpoolError(message)
            count += 1
            total += len(line)
            yield line[:-1]
        if count != self.record_count or total != self.bytes_written:
            message = "terminal report spool accounting is corrupt"
            raise ReportSpoolError(message)

    def close(self) -> None:
        """Release the owned handle; the OS owns removal of its backing storage.

        Raises:
            ReportSpoolError: If the handle cannot be closed.

        """
        if self.closed:
            return
        try:
            self.file.close()
        except OSError as error:
            message = "terminal report spool could not close its owned handle"
            raise ReportSpoolError(message) from error
        self.closed = True

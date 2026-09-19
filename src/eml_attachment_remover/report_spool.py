"""Private bounded storage for terminal schema-3 item records."""

from __future__ import annotations

import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Iterator

MAX_RECORD_BYTES: Final = 1024 * 1024
MAX_SPOOL_BYTES: Final = 64 * 1024 * 1024
PROJECT_ROOT: Final = Path(__file__).resolve().parents[2]


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
    if root.is_relative_to(PROJECT_ROOT):
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


@dataclass(slots=True)
class ReportSpool:
    """Own one private line-delimited terminal-report spool outside the repository."""

    path: Path
    bytes_written: int = 0
    record_count: int = 0
    closed: bool = False

    @classmethod
    def create(cls) -> ReportSpool:
        """Create one private external spool with a restrictive filesystem mode.

        Returns:
            The sole owner of a new empty report spool.

        """
        descriptor, raw_path = tempfile.mkstemp(
            prefix=".eml-attachment-remover-report-", dir=private_temp_root()
        )
        try:
            os.fchmod(descriptor, 0o600)
        finally:
            os.close(descriptor)
        return cls(Path(raw_path))

    def append(self, record: bytes) -> None:
        """Append one bounded complete JSON record or fail before accepting it.

        Raises:
            ReportSpoolError: If the record, spool state, or write result is unsafe.

        """
        if (
            self.closed
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
        descriptor = os.open(self.path, os.O_WRONLY | os.O_APPEND | os.O_CLOEXEC)
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
                try:
                    os.ftruncate(descriptor, self.bytes_written)
                except OSError as error:
                    message = "terminal report spool could not recover a partial record"
                    raise ReportSpoolError(message) from error
            os.close(descriptor)
        self.bytes_written += len(payload)
        self.record_count += 1

    def records(self) -> Iterator[bytes]:
        """Yield every complete bounded record after validating the spool shape.

        Yields:
            Each ordered complete JSON record without its newline delimiter.

        Raises:
            ReportSpoolError: If the owned spool is unavailable or malformed.

        """
        if self.closed or not self.path.is_file() or self.path.is_symlink():
            message = "terminal report spool is unavailable"
            raise ReportSpoolError(message)
        count = 0
        total = 0
        with self.path.open("rb") as source:
            while line := source.readline(MAX_RECORD_BYTES + 2):
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
        """Remove only the exact private spool owned by this report lifecycle.

        Raises:
            ReportSpoolError: If a trusted spool cannot be removed completely.

        """
        if self.closed:
            return
        if self.path.is_symlink() or not self.path.is_file():
            message = "terminal report spool is unsafe during cleanup"
            raise ReportSpoolError(message)
        self.path.unlink()
        if self.path.exists() or self.path.is_symlink():
            message = "terminal report spool cleanup was incomplete"
            raise ReportSpoolError(message)
        self.closed = True

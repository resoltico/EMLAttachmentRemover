"""Private report staging and one-shot delivery to the real output channels."""

from __future__ import annotations

import sys
import tempfile
import threading
from dataclasses import dataclass
from typing import IO, TYPE_CHECKING, Any, Final, cast

from .report_spool import private_temp_root

# JSON is ASCII and paths0 is written to the binary buffer; neither is re-encoded.
BYTE_CHANNEL: Final = "utf-8"
CHUNK_SIZE: Final = 64 * 1024
POLL_SECONDS: Final = 0.05

if TYPE_CHECKING:
    from collections.abc import Callable
    from contextlib import ExitStack
    from typing import TextIO

    from .cancellation import DeliveryGuard


@dataclass(frozen=True, slots=True)
class StagedChannels:
    """Private files that hold one complete report before it reaches a channel.

    They are created before any item can publish, so their allocation cannot fail
    after a destination became visible. Human text and diagnostics use their final
    channel's encoding, so escaping targets the channel they will actually reach;
    JSON and paths0 are byte channels and are staged unchanged.
    """

    out: TextIO
    err: TextIO

    @classmethod
    def open(cls, resources: ExitStack, output_format: str) -> StagedChannels:
        """Create both private staging files, owned by ``resources``.

        Returns:
            The staging pair for stdout and stderr.

        """
        root = str(private_temp_root())
        report_encoding = (
            _encoding(sys.stdout) if output_format == "human" else BYTE_CHANNEL
        )
        return cls(
            _staging_file(resources, report_encoding, root),
            _staging_file(resources, _encoding(sys.stderr), root),
        )

    def reset(self) -> None:
        """Discard any partial render so only one complete report is delivered."""
        for staged in (self.out, self.err):
            staged.seek(0)
            staged.truncate()

    def seal(self) -> None:
        """Make the staged report deliverable, or fail while recovery is still open.

        Everything that can fail before the first external byte happens here: the
        flush, the rewind, and a read of the first chunk from each file.
        """
        for staged in (self.out, self.err):
            staged.flush()
            staged.seek(0)
            staged.read(1)
            staged.seek(0)

    def deliver(self, output_format: str, guard: DeliveryGuard) -> None:
        """Copy the sealed diagnostics and report to the real channels, boundedly."""
        _bounded(lambda: self._copy(output_format, guard), guard)

    def _copy(self, output_format: str, guard: DeliveryGuard) -> None:
        """Write stderr then stdout in flushed chunks, stamping each one's progress."""
        _copy_chunks(self.err, sys.stderr, guard)
        if output_format in {"json", "paths0"}:
            _copy_chunks(self.out.buffer, sys.stdout.buffer, guard)
        else:
            _copy_chunks(self.out, sys.stdout, guard)


def _staging_file(resources: ExitStack, encoding: str, root: str) -> TextIO:
    """Create one private staging file in the encoding its channel expects.

    Returns:
        The open text file, closed and removed when ``resources`` exits.

    """
    # ruff: ignore[open-file-with-context-handler] - owned by the run's ExitStack.
    staged = tempfile.TemporaryFile(mode="w+", encoding=encoding, newline="", dir=root)
    return cast("TextIO", resources.enter_context(staged))


def _encoding(stream: object) -> str:
    """Return a channel's text encoding, or UTF-8 for an in-memory stand-in.

    Returns:
        The codec the channel will encode text with.

    """
    return getattr(stream, "encoding", None) or "utf-8"


def write_note(text: str, guard: DeliveryGuard) -> None:
    """Write one short diagnostic to standard error under the same bound."""

    def write() -> None:
        sys.stderr.write(text)
        sys.stderr.flush()
        guard.note_progress()

    _bounded(write, guard)


def _bounded(work: Callable[[], None], guard: DeliveryGuard) -> None:
    """Run channel output on a helper thread while this thread watches the guard.

    The delivering thread never blocks in a channel write, so signal handlers run on
    every platform and a stalled reader can be abandoned once the grace is spent.

    """
    failures: list[BaseException] = []

    def run() -> None:
        try:
            work()
        except BaseException as exc:  # ruff: ignore[blind-except] - re-raised on the delivering thread.
            failures.append(exc)

    worker = threading.Thread(target=run, daemon=True)
    worker.start()
    while worker.is_alive():
        worker.join(POLL_SECONDS)
        guard.check()
    if failures:
        raise failures[0]


def _copy_chunks(source: IO[Any], destination: IO[Any], guard: DeliveryGuard) -> None:
    """Copy a staged stream in bounded chunks, flushing and stamping each one."""
    while chunk := source.read(CHUNK_SIZE):
        destination.write(chunk)
        destination.flush()
        guard.note_progress()

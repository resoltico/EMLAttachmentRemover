"""Private report staging and one-shot delivery to the real output channels."""

from __future__ import annotations

import sys
import tempfile
from dataclasses import dataclass
from typing import TYPE_CHECKING, Final, cast

from .report_spool import private_temp_root

# JSON is ASCII and paths0 is written to the binary buffer; neither is re-encoded.
BYTE_CHANNEL: Final = "utf-8"

if TYPE_CHECKING:
    from contextlib import ExitStack
    from typing import TextIO


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

    def deliver(self, output_format: str) -> None:
        """Copy the staged diagnostics and report to the real channels."""
        self.err.seek(0)
        _copy_text(self.err, sys.stderr)
        self.out.flush()
        self.out.seek(0)
        if output_format in {"json", "paths0"}:
            _copy_bytes(self.out.buffer, sys.stdout.buffer)
        else:
            _copy_text(self.out, sys.stdout)


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


def _copy_text(source: object, destination: object) -> None:
    """Copy bounded staged text to one already-selected real output channel."""
    while chunk := source.read(1024 * 1024):  # type: ignore[attr-defined]
        destination.write(chunk)  # type: ignore[attr-defined]


def _copy_bytes(source: object, destination: object) -> None:
    """Copy bounded staged binary output to one already-selected real channel."""
    while chunk := source.read(1024 * 1024):  # type: ignore[attr-defined]
        destination.write(chunk)  # type: ignore[attr-defined]

"""One report's lifecycle: stage, seal, release, deliver once, then diagnose.

Every step that can fail runs before the first external byte, while the reserved
status evidence still allows a truthful recovery render. After delivery begins no
second document is ever produced.
"""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
from dataclasses import dataclass

from . import report_stream
from .cancellation import cancellation_name, delivery_guard
from .diagnostics import error_line
from .domain import AppError, BatchLedger, ExitCode
from .exit_status import exit_code
from .report_delivery import StagedChannels, write_note
from .reporting_v3 import report, write_human, write_json, write_paths0


@dataclass(slots=True)
class ReportSession:
    """Own one run's private staging and its single delivery."""

    channels: StagedChannels
    output_format: str
    mode: str
    delivery_started: bool = False

    def publish(self, ledger: BatchLedger, status: int) -> int:
        """Stage one complete report, release its spools, and deliver it once.

        Returns:
            The process status: the report's own, unless a signal arrived while the
            complete document was being delivered (then the interruption status).

        """
        status = self._stage(ledger, status)
        with delivery_guard() as guard:
            released = _release(ledger)
            self.delivery_started = True
            self.channels.deliver(self.output_format, guard)
            if released is not None:
                write_note(_cleanup_line(released), guard)
            if not guard.signals:
                return status
            name = cancellation_name(guard.signals[0])
            interrupted = AppError(
                ExitCode.INTERRUPTED,
                f"interrupted by {name} after the report was delivered",
            )
            write_note(error_line(interrupted), guard)
        return int(ExitCode.INTERRUPTED)

    def _stage(self, ledger: BatchLedger, status: int) -> int:
        """Render and seal the report, falling back to reserved status records.

        Returns:
            The status the staged report carries.

        Raises:
            OSError: If no truthful report can be staged at all.

        """
        try:
            self._render(ledger, status)
        except OSError:
            if ledger.emergency_report_spool is None or ledger.report_spool_failed:
                raise
            report_stream.recover(ledger)
            status = exit_code(ledger)
            self._render(ledger, status)
        return status

    def _render(self, ledger: BatchLedger, status: int) -> None:
        self.channels.reset()
        with (
            redirect_stdout(self.channels.out),
            redirect_stderr(self.channels.err),
        ):
            _write_selected(self.output_format, ledger, self.mode, status)
        self.channels.seal()


def _cleanup_line(error: OSError) -> str:
    return error_line(
        AppError(ExitCode.INTERNAL_ERROR, f"report cleanup failed: {error}")
    )


def _release(ledger: BatchLedger) -> OSError | None:
    """Release the report spools; a failure must not consume the staged report.

    Returns:
        The cleanup failure to diagnose after delivery, if any.

    """
    try:
        report_stream.close(ledger)
    except OSError as error:
        return error
    return None


def _write_selected(
    output_format: str, ledger: BatchLedger, mode: str, status: int
) -> None:
    """Write the selected channel from a spool, or from an in-memory ledger."""
    if ledger.report_spool is None:
        document = report(ledger, mode, status)
        if output_format == "json":
            write_json(document)
        elif output_format == "paths0":
            write_paths0(ledger)
        else:
            write_human(document)
    elif output_format == "json":
        report_stream.write_json(ledger, mode, status)
    elif output_format == "paths0":
        report_stream.write_paths0(ledger)
    else:
        report_stream.write_human(ledger)

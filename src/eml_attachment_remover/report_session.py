"""One report's lifecycle: stage, seal, release, deliver once, then diagnose.

Staging is validated before the first external byte, while retained terminal
receipts allow recovery independently of writable storage. After delivery begins
no second document is ever produced; output endpoint failures still propagate.
"""

from __future__ import annotations

from contextlib import ExitStack, redirect_stderr, redirect_stdout
from dataclasses import dataclass
from typing import TYPE_CHECKING

from . import report_emergency, report_stream
from .cancellation import (
    CancellationSignal,
    cancellation_name,
    checkpoint,
    delivery_guard,
)
from .diagnostics import error_line
from .domain import AppError, BatchLedger, ExitCode
from .exit_status import exit_code
from .report_batch_diagnostics import batch_error_line, interruption_line
from .report_delivery import StagedChannels, StagedReadError, write_note
from .report_document import (
    report,
    write_human,
    write_json,
    write_paths0,
)

if TYPE_CHECKING:
    from .cancellation import DeliveryGuard


@dataclass(slots=True)
class ReportSession:
    """Own one run's private staging and its single delivery."""

    channels: StagedChannels
    output_format: str
    mode: str
    delivery_started: bool = False
    delivery_complete: bool = False
    interruption_reported: bool = False

    def publish(self, ledger: BatchLedger, status: int) -> int:
        """Stage one complete report, release its spools, and deliver it once.

        Returns:
            The process status: the report's own, unless a signal arrived while the
            complete document was being delivered (then the interruption status).

        """
        with ExitStack() as resources:
            status = _acknowledge(ledger, status)
            prior_interruption = ledger.interruption
            status = self._stage(ledger, status, resources)
            selected = _acknowledge(ledger, status)
            if selected != status or ledger.interruption is not prior_interruption:
                status = selected
                status = self._stage(ledger, status, resources)
            return self._deliver(ledger, status, resources)

    def _deliver(self, ledger: BatchLedger, status: int, resources: ExitStack) -> int:
        """Deliver the selected sealed channels while retaining their owners.

        Returns:
            The processing or late interruption status.

        """
        with delivery_guard() as guard:
            try:
                status = self._deliver_or_recover(ledger, status, resources, guard)
                self.delivery_complete = True
            finally:
                self.delivery_started = bool(guard.report_units)
                released = _release(ledger)
            if released is not None:
                write_note(_cleanup_line(released), guard)
            if ledger.interruption is not None:
                if self.output_format != "json":
                    write_note(interruption_line(ledger.interruption.reason), guard)
                self.interruption_reported = True
        status = guard.result(status)
        if ledger.interruption is not None:
            return int(ExitCode.INTERRUPTED)
        if guard.signals:
            name = cancellation_name(guard.signals[0])
            interrupted = AppError(
                ExitCode.INTERRUPTED,
                f"interrupted by {name} after the report was delivered",
            )
            write_note(error_line(interrupted), guard)
            self.interruption_reported = True
        return status

    def _deliver_or_recover(
        self,
        ledger: BatchLedger,
        status: int,
        resources: ExitStack,
        guard: DeliveryGuard,
    ) -> int:
        """Recover a staged read only while stdout has accepted no report units.

        Returns:
            The status of the sole delivered report.

        Raises:
            StagedReadError: After partial output or without retained recovery evidence.

        """
        try:
            self.channels.deliver(self.output_format, guard)
        except StagedReadError:
            if guard.report_units or ledger.emergency_report_spool is None:
                raise
            diagnostics_sent = bool(guard.diagnostic_units)
            status = self._stage_recovery(ledger, resources)
            guard.check()
            self.channels.deliver(self.output_format, guard)
            if diagnostics_sent and self.output_format != "json":
                prefix = "" if guard.diagnostic_newline else "\n"
                write_note(prefix + batch_error_line(ledger.batch_error), guard)
        return status

    def _stage(self, ledger: BatchLedger, status: int, resources: ExitStack) -> int:
        """Render and seal the report, falling back to retained terminal receipts.

        Returns:
            The status the staged report carries.

        Raises:
            OSError: If no truthful report can be staged at all.

        """
        if ledger.interruption is not None and not all(
            item.archived for item in ledger.items
        ):
            return self._stage_memory(ledger, status, resources)
        if ledger.report_spool_failed and ledger.emergency_report_spool is not None:
            return self._stage_recovery(ledger, resources)
        try:
            self._render(ledger, status)
        except OSError:
            if ledger.emergency_report_spool is None:
                raise
            return self._stage_recovery(ledger, resources)
        return status

    def _stage_recovery(self, ledger: BatchLedger, resources: ExitStack) -> int:
        """Use fresh memory channels even after an interrupted recovery attempt.

        Returns:
            The status carried by the complete recovery report.

        """
        report_stream.recover(ledger)
        status = exit_code(ledger)
        return self._stage_memory(ledger, status, resources)

    def _stage_memory(
        self, ledger: BatchLedger, status: int, resources: ExitStack
    ) -> int:
        """Seal retained terminal outcomes without requiring any disk writes.

        Returns:
            The status preserved by the complete memory report.

        """
        self.channels.discard()
        self.channels = report_emergency.stage(
            resources, ledger, self.output_format, self.mode, status
        )
        return status

    def _render(self, ledger: BatchLedger, status: int) -> None:
        self.channels.reset()
        with (
            redirect_stdout(self.channels.out),
            redirect_stderr(self.channels.err),
        ):
            _write_selected(self.output_format, ledger, self.mode, status)
        self.channels.seal()


def _acknowledge(ledger: BatchLedger, status: int) -> int:
    """Observe requests while no external bytes can escape an incomplete report.

    Returns:
        The original status or the committed invocation interruption status.

    """
    try:
        checkpoint()
    except CancellationSignal as request:
        if ledger.interruption is None:
            ledger.record_interruption(request.name, "report")
        return int(ExitCode.INTERRUPTED)
    return status


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

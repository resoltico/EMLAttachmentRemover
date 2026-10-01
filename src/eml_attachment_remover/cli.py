"""Command entry point for schema-3 MIME-pruned batch processing."""

from __future__ import annotations

import signal
import sys
from contextlib import ExitStack
from dataclasses import dataclass

from . import report_emergency, report_stream
from .batch import BatchOptions, execute
from .cancellation import (
    CancellationSignal,
    DeliveryGuard,
    cancellation_name,
    checkpoint,
    delivery_guard,
    install_cancellation_handlers,
)
from .cli_intent import OutputIntent
from .cli_parser import (
    build_parser,
    validate_arguments,
)
from .diagnostics import error_line, write_error
from .domain import AppError, BatchLedger, ExitCode
from .exit_status import exit_code
from .native_paths import path_value
from .report_delivery import StagedChannels, write_note
from .report_diagnostics import bounded_message
from .report_session import ReportSession


@dataclass(slots=True)
class _RunState:
    """Own one run's ledger and its report session."""

    ledger: BatchLedger | None = None
    session: ReportSession | None = None
    intent: OutputIntent | None = None
    error_delivery: DeliveryGuard | None = None
    notice_started: bool = False


def _render_error(error: AppError, parser: object | None) -> int:
    """Render usage and ordinary failures without contaminating JSON stdout.

    Returns:
        The stable exit status encoded by the expected error.

    """
    if error.code is ExitCode.USAGE and parser is not None:
        parser.print_usage(sys.stderr)  # type: ignore[attr-defined]
    return write_error(error)


def _json_error(
    arguments: list[str],
    error: AppError,
    intent: OutputIntent | None = None,
    *,
    interrupted: CancellationSignal | None = None,
    state: _RunState | None = None,
) -> int:
    """Emit a schema-3 error document when raw argv selected JSON output.

    Returns:
        The stable exit status encoded by the expected error.

    """
    state = state or _RunState()
    state.error_delivery = DeliveryGuard()
    selected = intent or OutputIntent.inferred(arguments)
    ledger = BatchLedger.from_requests([
        path_value(source) for source in selected.sources
    ])
    ledger.batch_error = error
    if interrupted is not None:
        ledger.record_interruption(interrupted.name, "startup")
    else:
        ledger.finalize_not_run(
            "batch configuration error"
            if error.code is ExitCode.USAGE
            else "not run after report startup failure"
        )
    with ExitStack() as resources:
        channels = report_emergency.stage(
            resources, ledger, "json", selected.mode, int(error.code)
        )
        with delivery_guard() as guard:
            state.error_delivery = guard
            if interrupted is not None and not guard.signals:
                guard.record(interrupted.number, None)
            try:
                channels.deliver("json", guard)
            except BrokenPipeError:
                return 1
            if interrupted is not None:
                state.notice_started = True
        status = guard.result(int(error.code))
        first_late = int(interrupted is not None)
        if len(guard.signals) > first_late:
            state.notice_started = True
            name = cancellation_name(guard.signals[first_late])
            late_interruption = AppError(
                ExitCode.INTERRUPTED,
                f"interrupted by {name} after the report was delivered",
            )
            write_note(error_line(late_interruption), guard)
    return status


def main(argv: list[str] | None = None) -> int:
    """Run an input-order v3 batch and render exactly one selected channel.

    Returns:
        The final process status after reportable work is terminalized.

    """
    raw = list(sys.argv[1:] if argv is None else argv)
    return _dispatch(raw)


def _dispatch(raw: list[str]) -> int:
    state = _RunState(intent=OutputIntent.inferred(raw))
    try:
        with ExitStack() as resources:
            return _released(state, _guarded(raw, state, resources))
    except KeyboardInterrupt:
        return _finish_interruption(
            state, CancellationSignal(int(signal.SIGINT), "SIGINT")
        )
    except CancellationSignal as cancellation:
        return _finish_interruption(state, cancellation)


def _finish_interruption(state: _RunState, request: CancellationSignal) -> int:
    """Contain final CLI interruption, including a failed explanatory response.

    Returns:
        Status 130 even if a repeated request or unavailable channel prevents notice.

    """
    try:
        return _cancelled(state, request)
    except KeyboardInterrupt, CancellationSignal, Exception:  # ruff: ignore[blind-except] - never recursively render a failed interruption response.
        return int(ExitCode.INTERRUPTED)


def _guarded(raw: list[str], state: _RunState, resources: ExitStack) -> int:
    try:
        return _run(raw, state, resources)
    except CancellationSignal as cancellation:
        return _finish_interruption(state, cancellation)
    except AppError as error:
        return _application_error(raw, error, state.intent, state)
    except KeyboardInterrupt:
        return _finish_interruption(
            state, CancellationSignal(int(signal.SIGINT), "SIGINT")
        )
    except BrokenPipeError:
        return 1
    except Exception as exc:  # ruff: ignore[blind-except] - protects partial-batch reporting.
        return _internal_error(exc)


def _released(state: _RunState, status: int) -> int:
    """Release report spools after every rendering attempt has finished.

    Returns:
        The run's status; a cleanup failure is diagnosed but never rewrites a report
        that may already have been delivered.

    """
    if state.ledger is not None:
        try:
            report_stream.close(state.ledger)
        except OSError as error:
            _render_error(
                AppError(ExitCode.INTERNAL_ERROR, f"report cleanup failed: {error}"),
                None,
            )
    return status


def _cancelled(state: _RunState, cancellation: CancellationSignal) -> int:
    interrupted = AppError(ExitCode.INTERRUPTED, f"interrupted by {cancellation.name}")
    session = state.session
    if state.error_delivery is not None:
        if state.error_delivery.report_units:
            return _interruption_notice(state, cancellation)
        return _json_error(
            [], interrupted, state.intent, interrupted=cancellation, state=state
        )
    if (
        session is not None
        and session.delivery_complete
        and session.interruption_reported
    ):
        return int(ExitCode.INTERRUPTED)
    if (
        session is None
        and state.intent is not None
        and state.intent.output_format == "json"
    ):
        return _json_error(
            [], interrupted, state.intent, interrupted=cancellation, state=state
        )
    if (
        state.ledger is None
        or session is None
        or session.delivery_started
        or session.delivery_complete
    ):
        # Once delivery began, a second document would corrupt the channel.
        return _interruption_notice(state, cancellation)
    if state.ledger.interruption is None:
        state.ledger.record_interruption(cancellation.name, "report")
    # The final report is owed exactly once; further signals only wait behind it.
    with delivery_guard() as guard:
        status = session.publish(state.ledger, int(ExitCode.INTERRUPTED))
    return guard.result(status)


def _application_error(
    raw: list[str],
    error: AppError,
    intent: OutputIntent | None = None,
    state: _RunState | None = None,
) -> int:
    selected = intent or OutputIntent.inferred(raw)
    return (
        _json_error(raw, error, selected, state=state)
        if selected.output_format == "json"
        else _render_error(error, build_parser())
    )


def _interruption_notice(state: _RunState, request: CancellationSignal) -> int:
    """Attempt one safe diagnostic under a fresh bounded delivery guard.

    Returns:
        The interruption status; stdout is never touched.

    """
    if state.notice_started:
        return int(ExitCode.INTERRUPTED)
    state.notice_started = True
    line = error_line(
        AppError(
            ExitCode.INTERRUPTED, bounded_message(f"interrupted by {request.name}")
        )
    )
    encoding = sys.stderr.encoding or "utf-8"
    line = line.encode(encoding, "backslashreplace").decode(encoding)
    with delivery_guard() as guard:
        if not guard.signals:
            guard.record(request.number, None)
        write_note(line, guard)
    return guard.result(int(ExitCode.INTERRUPTED))


def _internal_error(error: Exception) -> int:
    message = str(error) or type(error).__name__
    return _render_error(AppError(ExitCode.INTERNAL_ERROR, message), None)


def _run(raw: list[str], state: _RunState, resources: ExitStack) -> int:
    parser = build_parser()
    namespace = parser.parse_args(raw)
    state.intent = OutputIntent.parsed(namespace)
    validate_arguments(namespace)
    options = BatchOptions(
        bool(namespace.dry_run),
        str(namespace.existing),
        bool(namespace.fail_fast),
        namespace.output,
        namespace.output_dir,
    )
    output_format = str(namespace.output_format)
    state.ledger = BatchLedger.from_requests([
        path_value(source) for source in namespace.source
    ])
    resources.callback(report_stream.close, state.ledger)
    try:
        channels = StagedChannels.open(resources, output_format)
    except OSError as error:
        raise AppError(
            ExitCode.WRITE_ERROR,
            f"could not allocate report staging: {error}",
            phase="report",
        ) from error
    session = ReportSession(
        channels,
        output_format,
        "dry-run" if options.dry_run else "apply",
    )
    state.session = session
    with install_cancellation_handlers():
        execute(list(namespace.source), options, ledger=state.ledger)
        checkpoint()
        return session.publish(state.ledger, exit_code(state.ledger))

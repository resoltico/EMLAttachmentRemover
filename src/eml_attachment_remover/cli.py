"""Command entry point for schema-3 MIME-pruned batch processing."""

from __future__ import annotations

import sys
from contextlib import ExitStack
from dataclasses import dataclass

from . import report_stream
from .batch import BatchOptions, execute
from .cancellation import (
    CancellationSignal,
    delivery_guard,
    install_cancellation_handlers,
)
from .cli_parser import (
    build_parser,
    raw_json_requested,
    raw_source_candidates,
    validate_arguments,
)
from .diagnostics import write_error
from .domain import AppError, BatchLedger, ExitCode
from .exit_status import exit_code
from .native_paths import path_value
from .report_delivery import StagedChannels
from .report_session import ReportSession
from .reporting_v3 import report, write_json


@dataclass(slots=True)
class _RunState:
    """Own one run's ledger and its report session."""

    ledger: BatchLedger | None = None
    session: ReportSession | None = None


def _render_error(error: AppError, parser: object | None) -> int:
    """Render usage and ordinary failures without contaminating JSON stdout.

    Returns:
        The stable exit status encoded by the expected error.

    """
    if error.code is ExitCode.USAGE and parser is not None:
        parser.print_usage(sys.stderr)  # type: ignore[attr-defined]
    return write_error(error)


def _json_error(arguments: list[str], error: AppError) -> int:
    """Emit a schema-3 error document when raw argv selected JSON output.

    Returns:
        The stable exit status encoded by the expected error.

    """
    sources = raw_source_candidates(arguments)
    ledger = BatchLedger.from_requests([path_value(source) for source in sources])
    ledger.batch_error = error
    ledger.finalize_not_run("batch configuration error")
    write_json(report(ledger, "apply", int(error.code)))
    return int(error.code)


def main(argv: list[str] | None = None) -> int:
    """Run an input-order v3 batch and render exactly one selected channel.

    Returns:
        The final process status after reportable work is terminalized.

    """
    raw = list(sys.argv[1:] if argv is None else argv)
    return _dispatch(raw)


def _dispatch(raw: list[str]) -> int:
    state = _RunState()
    with ExitStack() as resources:
        return _released(state, _guarded(raw, state, resources))


def _guarded(raw: list[str], state: _RunState, resources: ExitStack) -> int:
    try:
        return _run(raw, state, resources)
    except CancellationSignal as cancellation:
        return _cancelled(state, cancellation)
    except AppError as error:
        return _application_error(raw, error)
    except KeyboardInterrupt:
        return _render_error(AppError(ExitCode.INTERRUPTED, "interrupted"), None)
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
    if state.ledger is None or session is None or session.delivery_started:
        # Once delivery began, a second document would corrupt the channel.
        return _render_error(interrupted, None)
    if state.ledger.interruption is None:
        state.ledger.record_interruption(cancellation.name, "report")
    # The final report is owed exactly once; further signals only wait behind it.
    with delivery_guard():
        return session.publish(state.ledger, int(ExitCode.INTERRUPTED))


def _application_error(raw: list[str], error: AppError) -> int:
    return (
        _json_error(raw, error)
        if raw_json_requested(raw)
        else _render_error(error, build_parser())
    )


def _internal_error(error: Exception) -> int:
    message = str(error) or type(error).__name__
    return _render_error(AppError(ExitCode.INTERNAL_ERROR, message), None)


def _run(raw: list[str], state: _RunState, resources: ExitStack) -> int:
    parser = build_parser()
    namespace = parser.parse_args(raw)
    validate_arguments(namespace)
    options = BatchOptions(
        bool(namespace.dry_run),
        str(namespace.existing),
        bool(namespace.fail_fast),
        namespace.output,
        namespace.output_dir,
    )
    output_format = str(namespace.output_format)
    session = ReportSession(
        StagedChannels.open(resources, output_format),
        output_format,
        "dry-run" if options.dry_run else "apply",
    )
    state.session = session
    with install_cancellation_handlers():
        ledger = execute(list(namespace.source), options)
        state.ledger = ledger
        return session.publish(ledger, exit_code(ledger))

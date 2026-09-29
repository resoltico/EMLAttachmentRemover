"""Command entry point for schema-3 MIME-pruned batch processing."""

from __future__ import annotations

import sys
from contextlib import ExitStack, redirect_stderr, redirect_stdout
from dataclasses import dataclass
from typing import Final, cast

from . import report_stream
from .batch import BatchOptions, execute
from .cancellation import (
    CancellationSignal,
    defer_cancellation,
    install_cancellation_handlers,
)
from .cli_parser import (
    build_parser,
    raw_json_requested,
    raw_source_candidates,
    validate_arguments,
)
from .domain import (
    PROGRAM_NAME,
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    LedgerItem,
)
from .native_paths import path_value
from .report_delivery import StagedChannels
from .reporting_v3 import report, write_human, write_json, write_paths0

ACCEPTED: Final = frozenset({
    ItemStatus.CREATED,
    ItemStatus.EXISTING_VERIFIED,
    ItemStatus.WOULD_CREATE,
})


@dataclass(slots=True)
class _RunState:
    """Own one run's retained output request, report staging, and ledger."""

    ledger: BatchLedger | None = None
    output_format: str = "human"
    mode: str = "apply"
    staged: StagedChannels | None = None
    delivery_started: bool = False


def exit_code(ledger: BatchLedger) -> int:
    """Apply the documented terminal-state precedence to a completed ledger.

    Returns:
        The stable process code corresponding to the terminal ledger states.

    """
    if _is_interrupted(ledger):
        return int(ExitCode.INTERRUPTED)
    if (
        (
            ledger.batch_error is not None
            and ledger.batch_error.code is ExitCode.INTERNAL_ERROR
        )
        or _has_publication_error(ledger, ExitCode.INTERNAL_ERROR)
        or any(
            item.status is ItemStatus.FAILED
            and item.error is not None
            and item.error.code is ExitCode.INTERNAL_ERROR
            for item in ledger.items
        )
    ):
        return int(ExitCode.INTERNAL_ERROR)
    if ledger.batch_error is not None:
        return int(ledger.batch_error.code)
    if _has_status(ledger, ItemStatus.PUBLISHED_WITH_ERROR):
        return int(
            ExitCode.PUBLICATION_INCOMPLETE
            if len(ledger.items) == 1
            else ExitCode.BATCH_FAILURE
        )
    return _failure_exit(ledger)


def _is_interrupted(ledger: BatchLedger) -> bool:
    return (
        ledger.interruption is not None
        or _has_status(ledger, ItemStatus.CANCELLED)
        or _has_publication_error(ledger, ExitCode.INTERRUPTED)
    )


def _has_status(ledger: BatchLedger, status: ItemStatus) -> bool:
    return any(item.status is status for item in ledger.items)


def _has_publication_error(ledger: BatchLedger, code: ExitCode) -> bool:
    return any(
        item.status is ItemStatus.PUBLISHED_WITH_ERROR
        and item.error is not None
        and item.error.code is code
        for item in ledger.items
    )


def _failure_exit(ledger: BatchLedger) -> int:
    failed = [item for item in ledger.items if item.status is ItemStatus.FAILED]
    if not failed:
        return _remaining_exit(ledger)
    if len(ledger.items) != 1:
        return int(ExitCode.BATCH_FAILURE)
    return _single_failure_exit(failed[0])


def _remaining_exit(ledger: BatchLedger) -> int:
    return (
        int(ExitCode.BATCH_FAILURE)
        if any(item.status not in ACCEPTED for item in ledger.items)
        else int(ExitCode.SUCCESS)
    )


def _single_failure_exit(item: LedgerItem) -> int:
    return int(ExitCode.INTERNAL_ERROR if item.error is None else item.error.code)


def _render_error(error: AppError, parser: object | None) -> int:
    """Render usage and ordinary failures without contaminating JSON stdout.

    Returns:
        The stable exit status encoded by the expected error.

    """
    if error.code is ExitCode.USAGE and parser is not None:
        parser.print_usage(sys.stderr)  # type: ignore[attr-defined]
    sys.stderr.write(
        f"{PROGRAM_NAME}: error[{error.code.name}:{int(error.code)}]: {error.message}\n"
    )
    return int(error.code)


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
    status = int(ExitCode.INTERRUPTED)
    interrupted = AppError(ExitCode.INTERRUPTED, f"interrupted by {cancellation.name}")
    if state.ledger is None or state.delivery_started:
        # Once delivery began, a second document would corrupt the channel.
        return _render_error(interrupted, None)
    if state.ledger.interruption is None:
        state.ledger.record_interruption(cancellation.name, "report")
    return _write_then_close(state, status)


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
    state.output_format = str(namespace.output_format)
    state.mode = "dry-run" if options.dry_run else "apply"
    state.staged = StagedChannels.open(resources, state.output_format)
    with install_cancellation_handlers():
        ledger = execute(list(namespace.source), options)
        state.ledger = ledger
        return _write_then_close(state, exit_code(ledger))


def _write_selected(
    output_format: str, ledger: BatchLedger, mode: str, status: int
) -> None:
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


def _write_then_close(state: _RunState, status: int) -> int:
    """Stage one complete report, release its receipts, then deliver it once.

    Returns:
        The delivered report's status, which a staging failure can change.

    """
    ledger = cast("BatchLedger", state.ledger)
    staged = cast("StagedChannels", state.staged)
    if ledger.report_spool is None:
        state.delivery_started = True
        _write_selected(state.output_format, ledger, state.mode, status)
        return status
    staged.reset()
    try:
        with redirect_stdout(staged.out), redirect_stderr(staged.err):
            _write_selected(state.output_format, ledger, state.mode, status)
    except OSError:
        # Nothing has reached a channel yet: fall back to the reserved status
        # records rather than losing the outcome of already-published items.
        report_stream.recover(ledger)
        recovered = exit_code(ledger)
        state.delivery_started = True
        _write_selected(state.output_format, ledger, state.mode, recovered)
        return recovered
    report_stream.close(ledger)
    with defer_cancellation():
        state.delivery_started = True
        staged.deliver(state.output_format)
    return status

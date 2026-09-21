"""Command entry point for schema-3 MIME-pruned batch processing."""

from __future__ import annotations

import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import dataclass
from typing import Final

from . import report_stream
from .batch import BatchOptions, execute
from .cancellation import CancellationSignal, install_cancellation_handlers
from .cli_parser import (
    build_parser,
    raw_json_requested,
    raw_source_candidates,
    validate_arguments,
    validate_raw_arguments,
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
from .report_spool import private_temp_root
from .reporting_v3 import report, write_human, write_json, write_paths0

ACCEPTED: Final = frozenset({
    ItemStatus.CREATED,
    ItemStatus.EXISTING_VERIFIED,
    ItemStatus.WOULD_CREATE,
})


@dataclass(slots=True)
class _RunState:
    ledger: BatchLedger | None = None


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
    try:
        return _run(raw, state)
    except CancellationSignal as cancellation:
        return _cancelled(raw, state, cancellation)
    except AppError as error:
        return _application_error(raw, error)
    except KeyboardInterrupt:
        return _render_error(AppError(ExitCode.INTERRUPTED, "interrupted"), None)
    except BrokenPipeError:
        return 1
    except Exception as exc:  # ruff: ignore[blind-except] - protects partial-batch reporting.
        return _internal_error(exc)


def _cancelled(
    raw: list[str], state: _RunState, cancellation: CancellationSignal
) -> int:
    if state.ledger is None:
        return _render_error(
            AppError(ExitCode.INTERRUPTED, f"interrupted by {cancellation.name}"),
            None,
        )
    if state.ledger.interruption is None:
        state.ledger.record_interruption(cancellation.name, "report")
    status = int(ExitCode.INTERRUPTED)
    _write_then_close(
        "json" if raw_json_requested(raw) else "human",
        state.ledger,
        BatchOptions(
            dry_run=False,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
        status,
    )
    return status


def _application_error(raw: list[str], error: AppError) -> int:
    return (
        _json_error(raw, error)
        if raw_json_requested(raw)
        else _render_error(error, build_parser())
    )


def _internal_error(error: Exception) -> int:
    message = str(error) or type(error).__name__
    return _render_error(AppError(ExitCode.INTERNAL_ERROR, message), None)


def _run(raw: list[str], state: _RunState) -> int:
    parser = build_parser()
    validate_raw_arguments(raw)
    namespace = parser.parse_args(raw)
    validate_arguments(namespace)
    options = BatchOptions(
        bool(namespace.dry_run),
        str(namespace.existing),
        bool(namespace.fail_fast),
        namespace.output,
        namespace.output_dir,
    )
    with install_cancellation_handlers():
        ledger = execute(list(namespace.source), options)
        state.ledger = ledger
        status = exit_code(ledger)
        _write_then_close(namespace.output_format, ledger, options, status)
    return status


def _write_selected(
    output_format: str, ledger: BatchLedger, options: BatchOptions, status: int
) -> None:
    if ledger.report_spool is None:
        document = report(ledger, "dry-run" if options.dry_run else "apply", status)
        if output_format == "json":
            write_json(document)
        elif output_format == "paths0":
            write_paths0(ledger)
        else:
            write_human(document)
    elif output_format == "json":
        report_stream.write_json(
            ledger, "dry-run" if options.dry_run else "apply", status
        )
    elif output_format == "paths0":
        report_stream.write_paths0(ledger)
    else:
        report_stream.write_human(ledger)


def _copy_text(source: object, destination: object) -> None:
    """Copy bounded staged text to one already-selected real output channel."""
    while chunk := source.read(1024 * 1024):  # type: ignore[attr-defined]
        destination.write(chunk)  # type: ignore[attr-defined]


def _copy_bytes(source: object, destination: object) -> None:
    """Copy bounded staged binary output to one already-selected real channel."""
    while chunk := source.read(1024 * 1024):  # type: ignore[attr-defined]
        destination.write(chunk)  # type: ignore[attr-defined]


def _write_then_close(
    output_format: str, ledger: BatchLedger, options: BatchOptions, status: int
) -> None:
    """Stage a spooled report, clean its private receipt, then publish output once."""
    if ledger.report_spool is None:
        _write_selected(output_format, ledger, options, status)
        return
    closed = False
    try:
        with (
            tempfile.TemporaryFile(
                mode="w+", encoding="utf-8", newline="", dir=private_temp_root()
            ) as staged_out,
            tempfile.TemporaryFile(
                mode="w+", encoding="utf-8", newline="", dir=private_temp_root()
            ) as staged_err,
        ):
            with redirect_stdout(staged_out), redirect_stderr(staged_err):
                _write_selected(output_format, ledger, options, status)
            closed = True
            report_stream.close(ledger)
            staged_err.seek(0)
            _copy_text(staged_err, sys.stderr)
            staged_out.flush()
            staged_out.seek(0)
            if output_format in {"json", "paths0"}:
                _copy_bytes(staged_out.buffer, sys.stdout.buffer)
            else:
                _copy_text(staged_out, sys.stdout)
    finally:
        if not closed:
            report_stream.close(ledger)

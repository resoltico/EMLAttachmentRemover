"""Implement the command-line batch orchestration."""

from __future__ import annotations

import stat
import sys
from typing import TYPE_CHECKING

from .cli_boundary import handle_broken_pipe, quiet_error_status
from .cli_parser import (
    NO_ARGUMENTS_LINES,
    build_parser,
    json_output_requested,
    validate_cli_arguments,
)
from .models import (
    PROGRAM_NAME,
    ArgumentParser,
    BatchFailure,
    BatchOutcome,
    BatchSkip,
    CliError,
    ExitCode,
    OutputFormat,
    ProcessResult,
    _write_error,
    _write_line,
)
from .paths import (
    _absolute_path,
    _default_destination,
    _destination_exists,
    _paths_alias,
    _planned_outputs,
    _validate_output_directory,
    _validate_source,
)
from .processing import process_file
from .reporting import (
    _write_human_batch,
    _write_json_error,
    _write_json_report,
    _write_paths,
)

if TYPE_CHECKING:
    import argparse
    from pathlib import Path

_build_parser = build_parser
_json_output_requested = json_output_requested
_validate_cli_arguments = validate_cli_arguments


def _existing_destination_outcome(
    source: Path,
    destination: Path,
    *,
    dry_run: bool,
    skip_existing: bool,
) -> BatchFailure | BatchSkip | None:
    """Return a skip or failure for an already-existing destination.

    Returns:
        A skip, a destination-directory failure, or ``None`` when processing proceeds.

    Raises:
        CliError: If the existing entry cannot be inspected.

    """
    if dry_run or not skip_existing or not _destination_exists(destination):
        return None
    try:
        metadata = destination.lstat()
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not inspect existing output path {destination}: {exc}",
        ) from exc
    if not stat.S_ISREG(metadata.st_mode):
        return BatchFailure(
            source,
            ExitCode.OUTPUT_CONFLICT,
            f"existing output is not a regular non-symbolic file: {destination}",
        )
    if _paths_alias(source, destination):
        return BatchFailure(
            source,
            ExitCode.OUTPUT_CONFLICT,
            f"existing output aliases the source EML: {destination}",
        )
    return BatchSkip(source, _absolute_path(destination))


def _execute_plan(
    source: Path,
    destination: Path,
    *,
    dry_run: bool,
    force: bool,
    skip_existing: bool,
) -> ProcessResult | BatchFailure | BatchSkip:
    """Execute one plan and convert expected errors into batch data.

    Returns:
        A successful result, skip, or source-qualified failure.

    """
    try:
        validated_source = _validate_source(source)
        existing = _existing_destination_outcome(
            validated_source,
            destination,
            dry_run=dry_run,
            skip_existing=skip_existing,
        )
        if existing is not None:
            return existing
        return process_file(
            validated_source,
            destination,
            force=force,
            dry_run=dry_run,
        )
    except CliError as exc:
        return BatchFailure(source, exc.code, exc.message)


def _execute_plans(
    plans: list[tuple[Path, Path]],
    *,
    dry_run: bool,
    force: bool,
    skip_existing: bool,
    fail_fast: bool,
) -> BatchOutcome:
    """Execute prepared source-to-destination mappings.

    Returns:
        Successful, skipped, and failed inputs accumulated during execution.

    """
    results: list[ProcessResult] = []
    skips: list[BatchSkip] = []
    failures: list[BatchFailure] = []
    for source, destination in plans:
        outcome = _execute_plan(
            source,
            destination,
            dry_run=dry_run,
            force=force,
            skip_existing=skip_existing,
        )
        if isinstance(outcome, BatchFailure):
            failures.append(outcome)
            if fail_fast:
                break
        elif isinstance(outcome, BatchSkip):
            skips.append(outcome)
        else:
            results.append(outcome)
    return BatchOutcome(tuple(results), tuple(skips), tuple(failures))


def _write_failures(failures: tuple[BatchFailure, ...]) -> None:
    """Write batch failures to standard error."""
    for failure in failures:
        _write_error(
            sys.stderr,
            failure.code,
            f"{failure.source}: {failure.message}",
        )


def _report_outcome(
    outcome: BatchOutcome,
    output_format: OutputFormat,
    *,
    multiple: bool,
) -> None:
    """Render one completed batch in the selected output format."""
    results = list(outcome.results)
    skips = list(outcome.skips)
    failures = list(outcome.failures)
    if output_format is OutputFormat.JSON:
        _write_json_report(results, skips, failures)
    elif output_format is OutputFormat.PATHS:
        _write_paths(results, skips, nul_terminated=False)
        _write_failures(outcome.failures)
    elif output_format is OutputFormat.PATHS0:
        _write_paths(results, skips, nul_terminated=True)
        _write_failures(outcome.failures)
    else:
        _write_human_batch(results, skips, failures, multiple=multiple)


def _outcome_exit_code(
    outcome: BatchOutcome,
    *,
    source_count: int,
    fail_fast: bool,
) -> int:
    """Return the stable exit code for a completed batch.

    Returns:
        Zero, the first specific error, or the partial-batch error code.

    """
    if not outcome.failures:
        return int(ExitCode.SUCCESS)
    if source_count == 1 or fail_fast:
        return int(outcome.failures[0].code)
    return int(ExitCode.BATCH_FAILURE)


def _run_batch(arguments: argparse.Namespace) -> int:
    """Process all command-line inputs and return the appropriate exit code.

    Returns:
        The documented exit code for the completed batch.

    """
    sources: list[Path] = list(arguments.source)
    explicit_output: Path | None = arguments.output
    output_directory: Path | None = arguments.output_dir
    output_format: OutputFormat = arguments.output_format
    dry_run = bool(arguments.dry_run)
    force = bool(arguments.force)
    skip_existing = bool(arguments.skip_existing)
    fail_fast = bool(arguments.fail_fast)
    _validate_cli_arguments(
        sources,
        explicit_output,
        output_format,
        dry_run=dry_run,
    )
    if dry_run:
        normalized_sources = [_absolute_path(source) for source in sources]
        plans = [
            (source, _default_destination(source)) for source in normalized_sources
        ]
    else:
        if output_directory is not None:
            output_directory = _validate_output_directory(output_directory)
        plans = _planned_outputs(
            sources,
            explicit_output,
            output_directory,
        )
    outcome = _execute_plans(
        plans,
        dry_run=dry_run,
        force=force,
        skip_existing=skip_existing,
        fail_fast=fail_fast,
    )
    _report_outcome(outcome, output_format, multiple=len(sources) > 1)
    return _outcome_exit_code(
        outcome,
        source_count=len(sources),
        fail_fast=fail_fast,
    )


def _render_cli_error(
    error: CliError,
    *,
    json_requested: bool,
    parser: ArgumentParser | None = None,
    no_arguments: bool = False,
) -> None:
    """Render one error to the stream selected by the output format.

    Raises:
        RuntimeError: If a usage error has no parser for its usage text.

    """
    if json_requested:
        _write_json_error(error)
        return
    if no_arguments:
        for line in NO_ARGUMENTS_LINES:
            _write_line(sys.stderr, line)
        return
    if error.code is ExitCode.USAGE:
        if parser is None:
            message = "a usage error requires its argument parser"
            raise RuntimeError(message)
        parser.print_usage(sys.stderr)
    _write_error(sys.stderr, error.code, error.message)


def _write_cli_error(
    error: CliError,
    *,
    json_requested: bool,
    parser: ArgumentParser | None = None,
    no_arguments: bool = False,
) -> int:
    """Write one expected failure without escaping terminal I/O errors.

    Returns:
        The requested error status or a stable stream-failure status.

    """
    try:
        _render_cli_error(
            error,
            json_requested=json_requested,
            parser=parser,
            no_arguments=no_arguments,
        )
    except BrokenPipeError:
        return handle_broken_pipe()
    except OSError, UnicodeError:
        return quiet_error_status()
    return int(error.code)


def _write_interrupt(*, json_requested: bool) -> int:
    """Write an interruption report without escaping terminal I/O errors.

    Returns:
        The interruption status or a stable stream-failure status.

    """
    error = CliError(ExitCode.INTERRUPTED, "interrupted")
    try:
        if json_requested:
            _write_json_error(error)
        else:
            _write_line(sys.stderr, f"{PROGRAM_NAME}: interrupted")
    except BrokenPipeError:
        return handle_broken_pipe()
    except OSError, UnicodeError:
        return quiet_error_status()
    return int(error.code)


def main(argv: list[str] | None = None) -> int:
    """Run the command-line utility.

    Returns:
        The documented integer process exit code.

    """
    parser = _build_parser()
    raw_arguments = list(sys.argv[1:] if argv is None else argv)
    json_requested = _json_output_requested(raw_arguments)
    try:
        arguments = parser.parse_args(raw_arguments)
        return _run_batch(arguments)
    except BrokenPipeError:
        return handle_broken_pipe()
    except CliError as exc:
        return _write_cli_error(
            exc,
            json_requested=json_requested,
            parser=parser,
            no_arguments=not raw_arguments,
        )
    except KeyboardInterrupt:
        return _write_interrupt(json_requested=json_requested)
    except Exception as exc:  # ruff: ignore[blind-except] - Stable CLI failure boundary.
        message = str(exc) or type(exc).__name__
        error = CliError(ExitCode.INTERNAL_ERROR, message)
        return _write_cli_error(error, json_requested=json_requested)

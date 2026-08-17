"""Render processing reports for people and automation."""

from __future__ import annotations

import json
import os
import sys
from typing import Final, TextIO

from ._version import PROGRAM_VERSION
from .models import (
    PROGRAM_NAME,
    BatchFailure,
    BatchSkip,
    CliError,
    ProcessResult,
    RemovedPart,
    _format_mime_path,
    _write_error,
    _write_line,
)
from .transformation_models import (
    DiscardedBodyRepresentation,
    DiscardedBodyResource,
    SelectedPlainTextBody,
)

REPORT_SCHEMA_VERSION: Final = 2
REPORT_SCOPE: Final = "text-only"
type BodyRecord = SelectedPlainTextBody | DiscardedBodyRepresentation


def _write_body_records(
    stream: TextIO,
    label: str,
    records: tuple[BodyRecord, ...],
) -> None:
    """Write a count and source MIME path for body-level plan records."""
    _write_line(stream, f"{label}: {len(records)}")
    for record in records:
        _write_line(
            stream,
            f"  - {record.content_type} [MIME path {_format_mime_path(record.path)}]",
        )


def _write_file_records(
    stream: TextIO,
    label: str,
    records: tuple[RemovedPart, ...],
) -> None:
    """Write a count and source metadata for file-level plan records."""
    _write_line(stream, f"{label}: {len(records)}")
    for record in records:
        name = record.filename or "(unnamed MIME entity)"
        _write_line(
            stream,
            f"  - {name} [{record.content_type}; "
            f"MIME path {_format_mime_path(record.path)}]",
        )


def _write_discarded_resources(
    stream: TextIO,
    records: tuple[DiscardedBodyResource, ...],
) -> None:
    """Write discarded resource metadata and all source-body references."""
    _write_line(stream, f"Discarded body resources: {len(records)}")
    for record in records:
        name = record.filename or "(unnamed MIME entity)"
        references = ", ".join(_format_mime_path(path) for path in record.referenced_by)
        reference_text = references or "none"
        _write_line(
            stream,
            f"  - {name} [{record.content_type}; body references {reference_text}; "
            f"MIME path {_format_mime_path(record.path)}]",
        )


def _write_result(stream: TextIO, result: ProcessResult) -> None:
    """Write a concise human-readable report for one successful input."""
    heading = (
        "Text-only transformation plan."
        if result.dry_run
        else "Text-only transformation complete."
    )
    _write_line(stream, heading)
    _write_body_records(
        stream,
        "Selected plain-text bodies",
        result.selected_plain_text_bodies,
    )
    _write_body_records(
        stream,
        "Discarded body representations",
        result.discarded_body_representations,
    )
    _write_discarded_resources(stream, result.discarded_body_resources)
    _write_file_records(
        stream,
        "Removed ordinary attachments",
        result.removed_attachments,
    )
    if result.dry_run:
        _write_line(stream, "Dry run: no output file was written.")
    elif result.destination is not None and result.output_size is not None:
        _write_line(stream, f"Wrote: {result.destination}")
        _write_line(
            stream,
            f"Size: {result.source_size:,} -> {result.output_size:,} bytes",
        )
    for warning in result.warnings:
        _write_line(stream, f"Warning: {warning}")


def _removed_part_data(part: RemovedPart) -> dict[str, object]:
    """Convert one removed-part record to JSON-compatible data.

    Returns:
        A dictionary containing stable MIME metadata.

    """
    return {
        "content_type": part.content_type,
        "disposition": part.disposition,
        "filename": part.filename,
        "mime_path": _format_mime_path(part.path),
    }


def _body_record_data(part: BodyRecord) -> dict[str, object]:
    """Convert one body-plan record to JSON-compatible data.

    Returns:
        A dictionary containing stable source MIME metadata.

    """
    return {
        "content_type": part.content_type,
        "mime_path": _format_mime_path(part.path),
    }


def _discarded_resource_data(part: DiscardedBodyResource) -> dict[str, object]:
    """Convert one discarded body resource to JSON-compatible data.

    Returns:
        A dictionary containing stable source MIME metadata.

    """
    return {
        "content_type": part.content_type,
        "disposition": part.disposition,
        "filename": part.filename,
        "mime_path": _format_mime_path(part.path),
        "referenced_by": [_format_mime_path(path) for path in part.referenced_by],
    }


def _result_data(result: ProcessResult) -> dict[str, object]:
    """Convert one successful result to JSON-compatible data.

    Returns:
        A dictionary describing the source, output, removals, and warnings.

    """
    return {
        "destination": (
            str(result.destination) if result.destination is not None else None
        ),
        "dry_run": result.dry_run,
        "output_size": result.output_size,
        "discarded_body_representations": [
            _body_record_data(part) for part in result.discarded_body_representations
        ],
        "discarded_body_resources": [
            _discarded_resource_data(part) for part in result.discarded_body_resources
        ],
        "removed_attachments": [
            _removed_part_data(part) for part in result.removed_attachments
        ],
        "selected_plain_text_bodies": [
            _body_record_data(part) for part in result.selected_plain_text_bodies
        ],
        "source": str(result.source),
        "source_size": result.source_size,
        "status": "ok",
        "warnings": list(result.warnings),
    }


def _failure_data(failure: BatchFailure) -> dict[str, object]:
    """Convert one batch failure to JSON-compatible data.

    Returns:
        A dictionary containing its source, code, and message.

    """
    return {
        "error": {
            "code": int(failure.code),
            "message": failure.message,
            "name": failure.code.name,
        },
        "source": str(failure.source),
        "status": "error",
    }


def _skip_data(skip: BatchSkip) -> dict[str, object]:
    """Convert one skipped item to JSON-compatible data.

    Returns:
        A dictionary identifying the source and existing destination.

    """
    return {
        "destination": str(skip.destination),
        "source": str(skip.source),
        "status": "skipped",
    }


def _json_document(payload: dict[str, object]) -> str:
    """Return one canonical newline-terminated JSON document.

    Returns:
        ASCII-only, deterministically ordered, two-space-indented JSON.

    """
    return f"{json.dumps(payload, indent=2, sort_keys=True)}\n"


def _write_json_report(
    results: list[ProcessResult],
    skips: list[BatchSkip],
    failures: list[BatchFailure],
) -> None:
    """Write one machine-readable batch report to standard output."""
    payload = {
        "schema_version": REPORT_SCHEMA_VERSION,
        "scope": REPORT_SCOPE,
        "program": PROGRAM_NAME,
        "version": PROGRAM_VERSION,
        "ok": not failures,
        "results": [_result_data(result) for result in results],
        "skipped": [_skip_data(skip) for skip in skips],
        "errors": [_failure_data(failure) for failure in failures],
    }
    sys.stdout.write(_json_document(payload))
    sys.stdout.flush()


def _write_json_error(error: CliError) -> None:
    """Write one batch-level failure as a machine-readable JSON document."""
    payload = {
        "schema_version": REPORT_SCHEMA_VERSION,
        "scope": REPORT_SCOPE,
        "program": PROGRAM_NAME,
        "version": PROGRAM_VERSION,
        "ok": False,
        "results": [],
        "skipped": [],
        "errors": [
            {
                "status": "error",
                "source": None,
                "error": {
                    "name": error.code.name,
                    "code": int(error.code),
                    "message": error.message,
                },
            },
        ],
    }
    sys.stdout.write(_json_document(payload))
    sys.stdout.flush()


def _write_paths(
    results: list[ProcessResult],
    skips: list[BatchSkip],
    *,
    nul_terminated: bool,
) -> None:
    """Write successful and skipped output paths for automation consumers."""
    paths = [
        result.destination for result in results if result.destination is not None
    ] + [skip.destination for skip in skips]
    if nul_terminated:
        output = sys.stdout.buffer
        for path in paths:
            output.write(os.fsencode(path))
            output.write(b"\0")
        output.flush()
        return
    for path in paths:
        _write_line(sys.stdout, str(path))


def _write_human_batch(
    results: list[ProcessResult],
    skips: list[BatchSkip],
    failures: list[BatchFailure],
    *,
    multiple: bool,
) -> None:
    """Write human-readable batch results and failures."""
    for result_index, result in enumerate(results):
        if result_index:
            _write_line(sys.stdout, "")
        if multiple:
            _write_line(sys.stdout, f"Source: {result.source}")
        _write_result(sys.stdout, result)
    for skip_index, skip in enumerate(skips):
        if results or skip_index:
            _write_line(sys.stdout, "")
        _write_line(
            sys.stdout,
            f"Skipped unverified existing output for {skip.source}: {skip.destination}",
        )
    for failure in failures:
        _write_error(
            sys.stderr,
            failure.code,
            f"{failure.source}: {failure.message}",
        )

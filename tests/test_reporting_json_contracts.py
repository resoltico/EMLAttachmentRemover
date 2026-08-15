"""Lock the complete machine-readable reporting contract."""

from __future__ import annotations

import io
import json
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import PROGRAM_VERSION, models, reporting


def _records() -> tuple[
    models.ProcessResult,
    models.BatchSkip,
    models.BatchFailure,
]:
    """Return complete public synthetic reporting records.

    Returns:
        One success, skip, and failure containing every report field.

    """
    result = models.ProcessResult(
        source=Path("public-source.eml"),
        destination=Path("public-output.eml"),
        source_size=123,
        output_size=45,
        removed=(
            models.RemovedPart(
                path=(0, 2),
                content_type="application/octet-stream",
                filename=None,
                disposition="attachment",
            ),
        ),
        preserved_file_parts=(
            models.PreservedFilePart(
                path=(1,),
                content_type="application/pkcs7-mime",
                filename="public-signature.p7m",
                reason=models.KeepReason.SECURITY_ENTITY,
            ),
        ),
        warnings=("public warning",),
        dry_run=False,
    )
    skip = models.BatchSkip(Path("skip.eml"), Path("skip-output.eml"))
    failure = models.BatchFailure(
        Path("failed-ž.eml"),
        models.ExitCode.INPUT_ERROR,
        "public failure ž",
    )
    return result, skip, failure


def test_json_record_converters_preserve_every_exact_field() -> None:
    """Require exact nested keys and values before serialization."""
    result, skip, failure = _records()

    assert reporting._removed_part_data(result.removed[0]) == {  # ruff: ignore[private-member-access]
        "content_type": "application/octet-stream",
        "disposition": "attachment",
        "filename": None,
        "mime_path": "1.3",
    }
    assert reporting._preserved_part_data(  # ruff: ignore[private-member-access]
        result.preserved_file_parts[0]
    ) == {
        "content_type": "application/pkcs7-mime",
        "filename": "public-signature.p7m",
        "mime_path": "2",
        "reason": "cryptographic MIME entity",
    }
    assert reporting._result_data(result) == {  # ruff: ignore[private-member-access]
        "destination": "public-output.eml",
        "dry_run": False,
        "output_size": 45,
        "preserved": [
            {
                "content_type": "application/pkcs7-mime",
                "filename": "public-signature.p7m",
                "mime_path": "2",
                "reason": "cryptographic MIME entity",
            }
        ],
        "removed": [
            {
                "content_type": "application/octet-stream",
                "disposition": "attachment",
                "filename": None,
                "mime_path": "1.3",
            }
        ],
        "source": "public-source.eml",
        "source_size": 123,
        "status": "ok",
        "warnings": ["public warning"],
    }
    assert reporting._failure_data(failure) == {  # ruff: ignore[private-member-access]
        "error": {
            "code": 3,
            "message": "public failure ž",
            "name": "INPUT_ERROR",
        },
        "source": "failed-ž.eml",
        "status": "error",
    }
    assert reporting._skip_data(skip) == {  # ruff: ignore[private-member-access]
        "destination": "skip-output.eml",
        "source": "skip.eml",
        "status": "skipped",
    }


def test_json_batch_report_is_canonical_and_complete() -> None:
    """Require stable ordering, indentation, ASCII escaping, and a final newline."""
    result, skip, failure = _records()
    expected = {
        "version": PROGRAM_VERSION,
        "skipped": [reporting._skip_data(skip)],  # ruff: ignore[private-member-access]
        "results": [reporting._result_data(result)],  # ruff: ignore[private-member-access]
        "program": models.PROGRAM_NAME,
        "ok": False,
        "errors": [reporting._failure_data(failure)],  # ruff: ignore[private-member-access]
    }
    stdout = io.StringIO()

    with patch("sys.stdout", stdout):
        reporting._write_json_report(  # ruff: ignore[private-member-access]
            [result],
            [skip],
            [failure],
        )

    assert stdout.getvalue() == (
        json.dumps(expected, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    )
    assert "failed-\\u017e.eml" in stdout.getvalue()


def test_json_cli_error_is_canonical_and_complete() -> None:
    """Require the exact batch-shaped JSON schema for a command-level error."""
    error = models.CliError(models.ExitCode.USAGE, "public usage ž")
    expected = {
        "version": PROGRAM_VERSION,
        "skipped": [],
        "results": [],
        "program": models.PROGRAM_NAME,
        "ok": False,
        "errors": [
            {
                "status": "error",
                "source": None,
                "error": {
                    "name": "USAGE",
                    "code": 2,
                    "message": "public usage ž",
                },
            }
        ],
    }
    stdout = io.StringIO()

    with patch("sys.stdout", stdout):
        reporting._write_json_error(error)  # ruff: ignore[private-member-access]

    assert stdout.getvalue() == (
        json.dumps(expected, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    )
    assert "public usage \\u017e" in stdout.getvalue()


def test_human_removed_part_uses_exact_unnamed_placeholder() -> None:
    """Keep the unnamed-removal placeholder stable and case-sensitive."""
    result, _skip, _failure = _records()
    stream = io.StringIO()

    reporting._write_result(stream, result)  # ruff: ignore[private-member-access]

    assert "  - (unnamed MIME entity) [application/octet-stream;" in stream.getvalue()

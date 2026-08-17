"""Lock exact public console rendering and shared model behavior."""

from __future__ import annotations

import io
import os
import sys
from dataclasses import replace
from pathlib import Path
from unittest.mock import call, patch

from eml_attachment_remover import models, reporting, transformation_models


def _process_result(
    source: str,
    destination: str | None,
    *,
    dry_run: bool = False,
) -> models.ProcessResult:
    """Return one complete public synthetic processing result.

    Returns:
        A result containing every text-only plan, size, and warning detail.

    """
    return models.ProcessResult(
        source=Path(source),
        destination=None if destination is None else Path(destination),
        source_size=1_234,
        output_size=None if destination is None else 56,
        removed_attachments=(
            models.RemovedPart(
                path=(2,),
                content_type="application/octet-stream",
                filename="public.bin",
                disposition="attachment",
            ),
        ),
        selected_plain_text_bodies=(
            transformation_models.SelectedPlainTextBody((0,), "text/plain"),
        ),
        discarded_body_representations=(
            transformation_models.DiscardedBodyRepresentation(
                (1,),
                "multipart/related",
            ),
        ),
        discarded_body_resources=(
            transformation_models.DiscardedBodyResource(
                path=(1, 1),
                referenced_by=((1, 0),),
                content_type="image/jpeg",
                filename="public-image.jpg",
                disposition="inline",
            ),
        ),
        warnings=("public warning",),
        dry_run=dry_run,
    )


def test_mime_path_format_is_exact_for_root_and_nested_parts() -> None:
    """Expose stable one-based MIME paths in every report format."""
    format_path = models._format_mime_path  # ruff: ignore[private-member-access]

    assert format_path(()) == "root"
    assert format_path((0,)) == "1"
    assert format_path((0, 2, 9)) == "1.3.10"


def test_cli_error_preserves_exception_text_code_and_message() -> None:
    """Keep both ordinary exception behavior and structured CLI fields."""
    error = models.CliError(models.ExitCode.WRITE_ERROR, "public failure")

    assert str(error) == "public failure"
    assert error.args == ("public failure",)
    assert error.code is models.ExitCode.WRITE_ERROR
    assert error.message == "public failure"


def test_display_text_escapes_controls_and_unencodable_characters() -> None:
    """Preserve printable boundaries, DEL handling, and stream encoding safety."""
    raw = io.BytesIO()
    stream = io.TextIOWrapper(raw, encoding="ascii")
    try:
        displayed = models._display_text(  # ruff: ignore[private-member-access]
            "A\x00\x1f \x7f\x80\ufeffž",
            stream,
        )
    finally:
        stream.close()

    assert displayed == r"A\x00\x1f \x7f\x80\ufeff\u017e"


def test_write_line_uses_safe_text_and_one_real_newline() -> None:
    """Write exactly one escaped console line per call."""
    stream = io.StringIO()

    models._write_line(stream, "public\x00value")  # ruff: ignore[private-member-access]

    assert stream.getvalue() == "public\\x00value\n"


def test_write_error_has_one_stable_machine_readable_prefix() -> None:
    """Keep program name, symbolic code, numeric code, and message together."""
    stream = io.StringIO()

    models._write_error(  # ruff: ignore[private-member-access]
        stream,
        models.ExitCode.PARSE_ERROR,
        "public malformed MIME",
    )

    assert stream.getvalue() == (
        "remove-eml-attachments: error[PARSE_ERROR:5]: public malformed MIME\n"
    )


def test_nul_terminated_paths_use_exact_filesystem_bytes() -> None:
    """Write every successful or skipped path with one NUL byte separator."""
    raw = io.BytesIO()
    stream = io.TextIOWrapper(raw, encoding="utf-8")
    result = _process_result("public.eml", "public-output.eml")
    skip = models.BatchSkip(Path("skipped.eml"), Path("skipped-output.eml"))
    actual = b""
    try:
        with patch.object(sys, "stdout", stream):
            reporting._write_paths(  # ruff: ignore[private-member-access]
                [result],
                [skip],
                nul_terminated=True,
            )
        actual = raw.getvalue()
    finally:
        stream.close()

    assert actual == (
        os.fsencode(Path("public-output.eml"))
        + b"\0"
        + os.fsencode(Path("skipped-output.eml"))
        + b"\0"
    )


def test_human_result_report_is_exact_for_a_written_output() -> None:
    """Render the complete text-only plan and write details exactly."""
    stream = io.StringIO()

    reporting._write_result(  # ruff: ignore[private-member-access]
        stream,
        _process_result("public.eml", "public-output.eml"),
    )

    assert stream.getvalue() == (
        "Text-only transformation complete.\n"
        "Selected plain-text bodies: 1\n"
        "  - text/plain [MIME path 1]\n"
        "Discarded body representations: 1\n"
        "  - multipart/related [MIME path 2]\n"
        "Discarded body resources: 1\n"
        "  - public-image.jpg [image/jpeg; body references 2.1; MIME path 2.2]\n"
        "Removed ordinary attachments: 1\n"
        "  - public.bin [application/octet-stream; MIME path 3]\n"
        "Wrote: public-output.eml\n"
        "Size: 1,234 -> 56 bytes\n"
        "Warning: public warning\n"
    )


def test_human_result_report_is_exact_for_a_dry_run() -> None:
    """Render prospective action and suppress every write-only field."""
    stream = io.StringIO()

    reporting._write_result(  # ruff: ignore[private-member-access]
        stream,
        _process_result("public.eml", None, dry_run=True),
    )

    assert stream.getvalue() == (
        "Text-only transformation plan.\n"
        "Selected plain-text bodies: 1\n"
        "  - text/plain [MIME path 1]\n"
        "Discarded body representations: 1\n"
        "  - multipart/related [MIME path 2]\n"
        "Discarded body resources: 1\n"
        "  - public-image.jpg [image/jpeg; body references 2.1; MIME path 2.2]\n"
        "Removed ordinary attachments: 1\n"
        "  - public.bin [application/octet-stream; MIME path 3]\n"
        "Dry run: no output file was written.\n"
        "Warning: public warning\n"
    )


def test_human_discarded_resource_reports_an_empty_reference_set() -> None:
    """Distinguish related membership from an explicit body reference."""
    complete = _process_result("public.eml", "public-output.eml")
    resource = replace(complete.discarded_body_resources[0], referenced_by=())
    result = replace(complete, discarded_body_resources=(resource,))
    stream = io.StringIO()

    reporting._write_result(stream, result)  # ruff: ignore[private-member-access]

    assert "body references none; MIME path 2.2" in stream.getvalue()


def test_human_discarded_resource_reports_unnamed_and_multiple_references() -> None:
    """Render the exact fallback label and deterministic reference separator."""
    complete = _process_result("public.eml", "public-output.eml")
    resource = replace(
        complete.discarded_body_resources[0],
        filename=None,
        referenced_by=((0,), (1, 2)),
    )
    result = replace(complete, discarded_body_resources=(resource,))
    stream = io.StringIO()

    reporting._write_result(stream, result)  # ruff: ignore[private-member-access]

    assert (
        "  - (unnamed MIME entity) [image/jpeg; body references 1, 2.3; "
        "MIME path 2.2]\n"
    ) in stream.getvalue()


def test_human_result_requires_complete_output_metadata_for_write_details() -> None:
    """Suppress write-only fields if either destination detail is unavailable."""
    complete = _process_result("public.eml", "public-output.eml")

    for result in (
        replace(complete, destination=None),
        replace(complete, output_size=None),
    ):
        stream = io.StringIO()

        reporting._write_result(  # ruff: ignore[private-member-access]
            stream,
            result,
        )

        report = stream.getvalue()
        assert "Wrote:" not in report
        assert "Size:" not in report


def test_skip_data_is_an_exact_machine_readable_contract() -> None:
    """Retain every documented skip key and value without extras."""
    skip = models.BatchSkip(Path("public.eml"), Path("public-output.eml"))

    actual = reporting._skip_data(skip)  # ruff: ignore[private-member-access]

    assert actual == {
        "destination": "public-output.eml",
        "source": "public.eml",
        "status": "skipped",
    }


def test_human_batch_preserves_section_order_and_separators() -> None:
    """Render exact transitions among results, skips, and failures."""
    first = _process_result("one.eml", "one-output.eml")
    second = _process_result("two.eml", "two-output.eml")
    first_skip = models.BatchSkip(Path("three.eml"), Path("three-output.eml"))
    second_skip = models.BatchSkip(Path("four.eml"), Path("four-output.eml"))
    failure = models.BatchFailure(
        Path("five.eml"),
        models.ExitCode.INPUT_ERROR,
        "public missing",
    )
    with (
        patch.object(reporting, "_write_line") as write_line,
        patch.object(reporting, "_write_result") as write_result,
        patch.object(reporting, "_write_error") as write_error,
    ):
        reporting._write_human_batch(  # ruff: ignore[private-member-access]
            [first, second],
            [first_skip, second_skip],
            [failure],
            multiple=True,
        )

    assert write_line.call_args_list == [
        call(sys.stdout, "Source: one.eml"),
        call(sys.stdout, ""),
        call(sys.stdout, "Source: two.eml"),
        call(sys.stdout, ""),
        call(
            sys.stdout,
            "Skipped unverified existing output for three.eml: three-output.eml",
        ),
        call(sys.stdout, ""),
        call(
            sys.stdout,
            "Skipped unverified existing output for four.eml: four-output.eml",
        ),
    ]
    assert write_result.call_args_list == [
        call(sys.stdout, first),
        call(sys.stdout, second),
    ]
    write_error.assert_called_once_with(
        sys.stderr,
        models.ExitCode.INPUT_ERROR,
        "five.eml: public missing",
    )

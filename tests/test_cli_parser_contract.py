"""Lock the public command-line grammar and in-process orchestration contract."""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import call, patch

import pytest

from eml_attachment_remover import PROGRAM_VERSION, cli, models
from tests.test_support import EXPECTED_NO_ARGUMENTS, run_cli

if TYPE_CHECKING:
    from _pytest.capture import CaptureFixture

EXPECTED_HELP = """\
usage: remove-eml-attachments [-h] [-o OUTPUT | --output-dir OUTPUT_DIR] [-f |
                              --skip-existing] [--dry-run] [--fail-fast]
                              [--output-format {human,json,paths,paths0}]
                              [--version]
                              source [source ...]

Create verified text-only EML working copies by retaining a safe plain-text
body and discarding HTML alternatives, embedded body resources, and ordinary
attachments.

positional arguments:
  source                one or more source EML files

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT   destination EML; valid only with one source
  --output-dir OUTPUT_DIR
                        existing directory for all generated EML files
  -f, --force           replace existing destinations, never any selected
                        source file
  --skip-existing       leave existing destination files unchanged and report
                        them as skipped
  --dry-run             report the text-only transformation plan without
                        writing output files
  --fail-fast           stop at the first failed input instead of completing
                        the batch
  --output-format {human,json,paths,paths0}
                        report format: human, json, newline-delimited paths,
                        or NUL-delimited paths0 (default: human)
  --version             show program's version number and exit

Exit codes: 0 success; 2 usage; 3 input; 4 output conflict; 5 MIME parse; 6
text-only transformation unavailable; 7 write; 8 verification; 9 partial batch
failure; 70 internal; 130 interrupted. For a filename beginning with '-',
place '--' before the filenames.
"""


def test_parser_help_is_an_exact_public_contract() -> None:
    """Expose every option, spelling, description, and exit-code summary."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    assert parser.format_help() == EXPECTED_HELP


def test_no_arguments_show_the_exact_first_run_guidance() -> None:
    """Present a concise first-run explanation without version noise."""
    result = run_cli()

    assert result.returncode == int(models.ExitCode.USAGE)
    assert not result.stdout
    assert result.stderr == EXPECTED_NO_ARGUMENTS
    assert PROGRAM_VERSION not in result.stderr


def test_parser_default_namespace_is_typed_and_complete() -> None:
    """Parse the minimum invocation into stable typed defaults."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    assert parser.allow_abbrev is False

    arguments = parser.parse_args(["public.eml"])

    assert vars(arguments) == {
        "dry_run": False,
        "fail_fast": False,
        "force": False,
        "output": None,
        "output_dir": None,
        "output_format": models.OutputFormat.HUMAN,
        "skip_existing": False,
        "source": [Path("public.eml")],
    }
    assert arguments.output_format is models.OutputFormat.HUMAN


def test_parser_accepts_every_value_bearing_and_boolean_option() -> None:
    """Preserve option actions, aliases, conversions, and nondefault values."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    arguments = parser.parse_args([
        "-f",
        "--dry-run",
        "--fail-fast",
        "--output-format",
        "json",
        "-o",
        "result.eml",
        "public.eml",
    ])

    assert vars(arguments) == {
        "dry_run": True,
        "fail_fast": True,
        "force": True,
        "output": Path("result.eml"),
        "output_dir": None,
        "output_format": models.OutputFormat.JSON,
        "skip_existing": False,
        "source": [Path("public.eml")],
    }
    assert arguments.output_format is models.OutputFormat.JSON


def test_parser_accepts_batch_output_directory_and_skip_mode() -> None:
    """Preserve the alternate mutually exclusive options and multiple sources."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    arguments = parser.parse_args([
        "--output-dir",
        "public-output",
        "--skip-existing",
        "first.eml",
        "second.eml",
    ])

    assert vars(arguments) == {
        "dry_run": False,
        "fail_fast": False,
        "force": False,
        "output": None,
        "output_dir": Path("public-output"),
        "output_format": models.OutputFormat.HUMAN,
        "skip_existing": True,
        "source": [Path("first.eml"), Path("second.eml")],
    }


def test_parser_rejects_missing_source_with_typed_exact_error() -> None:
    """Require at least one source through the custom usage-error boundary."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    with pytest.raises(models.CliError) as raised:
        parser.parse_args([])

    assert raised.value.code is models.ExitCode.USAGE
    assert raised.value.message == "the following arguments are required: source"
    assert str(raised.value) == raised.value.message


def test_parser_version_is_exact_public_output(
    capsys: CaptureFixture[str],
) -> None:
    """Keep the parser-owned version option and its stable rendering."""
    parser = cli._build_parser()  # ruff: ignore[private-member-access]

    with pytest.raises(SystemExit) as raised:
        parser.parse_args(["--version"])

    assert raised.value.code == 0
    captured = capsys.readouterr()
    assert captured.out == f"remove-eml-attachments {PROGRAM_VERSION}\n"
    assert not captured.err


def test_cli_argument_validation_accepts_every_legal_boundary() -> None:
    """Do not reject ordinary single, batch, human, or JSON invocations."""
    validate = cli._validate_cli_arguments  # ruff: ignore[private-member-access]

    validate(
        [Path("one.eml")],
        Path("out.eml"),
        models.OutputFormat.HUMAN,
        dry_run=False,
    )
    validate(
        [Path("one.eml"), Path("two.eml")],
        None,
        models.OutputFormat.JSON,
        dry_run=True,
    )
    validate(
        [Path("one.eml")],
        None,
        models.OutputFormat.PATHS,
        dry_run=False,
    )


def test_cli_argument_validation_reports_exact_illegal_combinations() -> None:
    """Retain exact usage diagnostics for ambiguous and contradictory requests."""
    validate = cli._validate_cli_arguments  # ruff: ignore[private-member-access]

    with pytest.raises(models.CliError) as multiple:
        validate(
            [Path("one.eml"), Path("two.eml")],
            Path("out.eml"),
            models.OutputFormat.HUMAN,
            dry_run=False,
        )
    with pytest.raises(models.CliError) as dry_paths:
        validate(
            [Path("one.eml")],
            None,
            models.OutputFormat.PATHS0,
            dry_run=True,
        )

    assert (multiple.value.code, multiple.value.message) == (
        models.ExitCode.USAGE,
        "--output may be used only when exactly one source file is supplied",
    )
    assert (dry_paths.value.code, dry_paths.value.message) == (
        models.ExitCode.USAGE,
        "path-only output formats cannot be combined with --dry-run",
    )


@pytest.mark.parametrize(
    ("arguments", "expected"),
    [
        (["--output-format=json"], True),
        (["--output-format", "json"], True),
        (["--output-format=decoy=json"], False),
        (["--output-format", "json", "--"], True),
        (["--", "--output-format=json"], False),
        (["--output-format", "--"], False),
        (["public.eml", "--output-format", "human"], False),
        (["--output-format"], False),
        (["--OUTPUT-FORMAT", "json"], False),
        ([], False),
    ],
)
def test_json_output_detection_accepts_only_documented_syntax(
    arguments: list[str],
    expected: object,
) -> None:
    """Recognize both documented JSON forms without near-option matches."""
    actual = cli._json_output_requested(  # ruff: ignore[private-member-access]
        arguments,
    )

    assert actual is expected


def test_main_uses_only_arguments_after_the_program_name() -> None:
    """Honor ``sys.argv`` and pass the parsed namespace to batch execution."""
    parsed = object()
    with (
        patch.object(sys, "argv", ["program", "--dry-run", "public.eml"]),
        patch.object(cli, "_build_parser") as build_parser,
        patch.object(cli, "_json_output_requested", return_value=False) as detect_json,
        patch.object(cli, "_run_batch", return_value=17) as run_batch,
    ):
        build_parser.return_value.parse_args.return_value = parsed
        status = cli.main(None)

    build_parser.return_value.parse_args.assert_called_once_with([
        "--dry-run",
        "public.eml",
    ])
    detect_json.assert_called_once_with(["--dry-run", "public.eml"])
    run_batch.assert_called_once_with(parsed)
    assert status == 17


def test_main_preserves_internal_error_details_and_empty_fallbacks() -> None:
    """Render an exception message, or its type when the message is empty."""
    failures = (RuntimeError("public detail"), RuntimeError())
    for failure, expected in zip(
        failures, ("public detail", "RuntimeError"), strict=True
    ):
        with (
            patch.object(cli, "_build_parser") as build_parser,
            patch.object(cli, "_json_output_requested", return_value=False),
            patch.object(cli, "_run_batch", side_effect=failure),
            patch.object(cli, "_write_error") as write_error,
        ):
            build_parser.return_value.parse_args.return_value = object()
            status = cli.main(["public.eml"])

        write_error.assert_called_once_with(
            sys.stderr,
            models.ExitCode.INTERNAL_ERROR,
            expected,
        )
        assert status == int(models.ExitCode.INTERNAL_ERROR)


def test_failure_writer_preserves_every_error_field_and_order() -> None:
    """Send exact source-qualified failures to the standardized renderer."""
    failures = (
        models.BatchFailure(Path("one.eml"), models.ExitCode.INPUT_ERROR, "missing"),
        models.BatchFailure(Path("two.eml"), models.ExitCode.PARSE_ERROR, "invalid"),
    )
    with patch.object(cli, "_write_error") as write_error:
        cli._write_failures(failures)  # ruff: ignore[private-member-access]

    assert write_error.call_args_list == [
        call(sys.stderr, models.ExitCode.INPUT_ERROR, "one.eml: missing"),
        call(sys.stderr, models.ExitCode.PARSE_ERROR, "two.eml: invalid"),
    ]


def test_existing_destination_checks_preserve_short_circuits_and_failure() -> None:
    """Distinguish dry-run, no-skip, and directory-conflict outcomes."""
    source = Path("public.eml")
    destination = Path("public-output.eml")
    existing = cli._existing_destination_outcome  # ruff: ignore[private-member-access]
    with patch.object(cli, "_destination_exists", return_value=True) as exists:
        assert existing(source, destination, dry_run=True, skip_existing=True) is None
        assert existing(source, destination, dry_run=False, skip_existing=False) is None
    exists.assert_not_called()
    with (
        patch.object(cli, "_destination_exists", return_value=True),
        patch.object(Path, "lstat") as lstat,
    ):
        lstat.return_value.st_mode = 0o040755
        outcome = existing(
            source,
            destination,
            dry_run=False,
            skip_existing=True,
        )
    assert outcome == models.BatchFailure(
        source,
        models.ExitCode.OUTPUT_CONFLICT,
        "existing output is not a regular non-symbolic file: public-output.eml",
    )


def test_execute_plans_preserves_arguments_results_skips_and_failures() -> None:
    """Keep exact orchestration calls and ordered immutable batch results."""
    sources = [Path("one.eml"), Path("two.eml"), Path("three.eml")]
    destinations = [Path("one.out"), Path("two.out"), Path("three.out")]
    result = models.ProcessResult(
        source=sources[0],
        destination=destinations[0],
        source_size=10,
        output_size=5,
        removed_attachments=(),
        selected_plain_text_bodies=(),
        discarded_body_representations=(),
        discarded_body_resources=(),
        warnings=(),
        dry_run=False,
    )
    skip = models.BatchSkip(sources[1], destinations[1])
    failure = models.BatchFailure(
        sources[2],
        models.ExitCode.OUTPUT_CONFLICT,
        "public conflict",
    )
    with (
        patch.object(
            cli,
            "_existing_destination_outcome",
            side_effect=(None, skip, failure),
        ) as existing,
        patch.object(cli, "process_file", return_value=result) as process_file,
        patch.object(cli, "_validate_source", side_effect=lambda source: source),
    ):
        outcome = cli._execute_plans(  # ruff: ignore[private-member-access]
            list(zip(sources, destinations, strict=True)),
            dry_run=False,
            force=True,
            skip_existing=True,
            fail_fast=False,
        )

    assert existing.call_args_list == [
        call(source, destination, dry_run=False, skip_existing=True)
        for source, destination in zip(sources, destinations, strict=True)
    ]
    process_file.assert_called_once_with(
        sources[0],
        destinations[0],
        force=True,
        dry_run=False,
    )
    assert outcome == models.BatchOutcome((result,), (skip,), (failure,))

"""Exact parser receipts for the deliberately narrow v3 command line."""

from __future__ import annotations

import pytest

from eml_attachment_remover.cli_parser import (
    MIGRATION_EXISTING,
    MIGRATION_PATHS,
    build_parser,
    raw_json_requested,
    raw_source_candidates,
    validate_arguments,
    validate_raw_arguments,
)
from eml_attachment_remover.domain import PROGRAM_NAME, AppError, ExitCode


def test_parser_declares_the_complete_breaking_v3_surface() -> None:
    parser = build_parser()
    actions = {action.dest: action for action in parser._actions}  # ruff: ignore[private-member-access] - action metadata is an observable CLI receipt.
    assert (parser.prog, parser.allow_abbrev) == (PROGRAM_NAME, False)
    assert tuple(actions) == (
        "help",
        "source",
        "output",
        "output_dir",
        "existing",
        "dry_run",
        "fail_fast",
        "output_format",
        "version",
    )
    assert (
        actions["source"].option_strings,
        actions["source"].nargs,
        actions["source"].required,
    ) == ([], "+", True)
    assert (
        actions["output"].option_strings,
        actions["output_dir"].option_strings,
        actions["existing"].choices,
        actions["existing"].default,
        actions["output_format"].choices,
        actions["output_format"].default,
    ) == (
        ["-o", "--output"],
        ["--output-dir"],
        ("error", "verify"),
        "error",
        ("human", "json", "paths0"),
        "human",
    )
    assert (
        actions["dry_run"].option_strings,
        actions["dry_run"].nargs,
        actions["dry_run"].default,
        actions["fail_fast"].option_strings,
        actions["fail_fast"].nargs,
        actions["fail_fast"].default,
        actions["version"].option_strings,
        actions["version"].nargs,
    ) == (
        ["--dry-run"],
        0,
        False,
        ["--fail-fast"],
        0,
        False,
        ["--version"],
        0,
    )


def test_parser_receives_every_supported_value_and_boolean_option() -> None:
    namespace = build_parser().parse_args([
        "-o",
        "copy.eml",
        "--existing=verify",
        "--dry-run",
        "--fail-fast",
        "--output-format=json",
        "source.eml",
    ])
    assert vars(namespace) == {
        "source": ["source.eml"],
        "output": "copy.eml",
        "output_dir": None,
        "existing": "verify",
        "dry_run": True,
        "fail_fast": True,
        "output_format": "json",
    }


@pytest.mark.parametrize(
    ("arguments", "message"),
    [
        (["--out", "copy.eml", "source.eml"], "unrecognized arguments: --out"),
        (
            ["--existing", "old", "source.eml"],
            (
                "argument --existing: invalid choice: 'old' "
                "(choose from 'error', 'verify')"
            ),
        ),
        (
            ["--output", "copy.eml", "--output-dir", "copies", "source.eml"],
            "argument --output-dir: not allowed with argument -o/--output",
        ),
    ],
)
def test_parser_rejects_abbreviation_invalid_values_and_competing_outputs(
    arguments: list[str], message: str
) -> None:
    parser = build_parser()
    with pytest.raises(AppError) as captured:
        parser.parse_args(arguments)
    assert captured.value.code is ExitCode.USAGE
    assert captured.value.message == message


def test_removed_spelling_receipts_cover_aliases_assignments_and_marker() -> None:
    for argument in (
        "--force",
        "--force=yes",
        "-f",
        "-fanything",
        "--skip-existing",
        "--skip-existing=yes",
    ):
        with pytest.raises(AppError) as captured:
            validate_raw_arguments([argument, "source.eml"])
        assert (captured.value.code, captured.value.message) == (
            ExitCode.USAGE,
            MIGRATION_EXISTING,
        )
    validate_raw_arguments(["--", "--force", "source.eml"])


def test_removed_paths_receipts_distinguish_paths0_and_end_of_options() -> None:
    for arguments in (
        ["--output-format=paths", "source.eml"],
        ["--output-format", "paths", "source.eml"],
    ):
        with pytest.raises(AppError) as captured:
            validate_raw_arguments(arguments)
        assert (captured.value.code, captured.value.message) == (
            ExitCode.USAGE,
            MIGRATION_PATHS,
        )
    validate_raw_arguments(["--output-format=paths0", "source.eml"])
    validate_raw_arguments(["--", "--output-format=paths", "source.eml"])


def test_preparse_json_and_source_receipts_honor_consumption_and_marker() -> None:
    consumed = [
        "-o",
        "copy.eml",
        "--output-dir",
        "copies",
        "--existing",
        "verify",
        "--output-format",
        "json",
        "source.eml",
    ]
    assert raw_json_requested(consumed) is True
    assert raw_source_candidates(consumed) == ["source.eml"]
    assert raw_json_requested(["--output-format=json", "source.eml"]) is True
    assert raw_json_requested(["--output-format", "human", "source.eml"]) is False
    assert raw_json_requested(["--", "--output-format=json"]) is False
    assert raw_source_candidates(["-o"]) == []
    assert raw_source_candidates(["--", "-source.eml", "source.eml"]) == [
        "-source.eml",
        "source.eml",
    ]


def test_postparse_validation_preserves_exact_v3_boundaries() -> None:
    parser = build_parser()
    validate_arguments(
        parser.parse_args(["--output-dir", "copies", "one.eml", "two.eml"])
    )
    for arguments, message in (
        (
            ["--output", "copy.eml", "one.eml", "two.eml"],
            "--output may be used only with exactly one source",
        ),
        (
            ["--dry-run", "--output-format", "paths0", "one.eml"],
            "--output-format=paths0 cannot be used with --dry-run",
        ),
    ):
        with pytest.raises(AppError) as captured:
            validate_arguments(parser.parse_args(arguments))
        assert (captured.value.code, captured.value.message) == (
            ExitCode.USAGE,
            message,
        )

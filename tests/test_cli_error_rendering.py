# ruff: file-ignore[private-member-access]
"""Exact contracts for CLI error classification and rendering."""

from __future__ import annotations

import sys
from unittest.mock import MagicMock, call, patch

import pytest

from eml_attachment_remover import cli, models
from eml_attachment_remover.cli_parser import NO_ARGUMENTS_LINES


def test_cli_error_rendering_uses_only_the_documented_streams() -> None:
    parser = MagicMock(spec=models.ArgumentParser)
    usage = models.CliError(models.ExitCode.USAGE, "public usage")
    ordinary = models.CliError(models.ExitCode.INPUT_ERROR, "public input")
    with patch.object(cli, "_write_error") as write_error:
        cli._render_cli_error(usage, json_requested=False, parser=parser)
        parser.print_usage.assert_called_once_with(sys.stderr)
        parser.print_usage.reset_mock()
        cli._render_cli_error(ordinary, json_requested=False, parser=parser)

    parser.print_usage.assert_not_called()
    assert write_error.call_args_list == [
        call(sys.stderr, models.ExitCode.USAGE, "public usage"),
        call(sys.stderr, models.ExitCode.INPUT_ERROR, "public input"),
    ]


def test_no_argument_rendering_uses_the_safe_line_writer() -> None:
    """Render the first-run screen without the generic parser usage or label."""
    parser = MagicMock(spec=models.ArgumentParser)
    usage = models.CliError(models.ExitCode.USAGE, "parser-owned detail")

    with patch.object(cli, "_write_line") as write_line:
        cli._render_cli_error(
            usage,
            json_requested=False,
            parser=parser,
            no_arguments=True,
        )

    parser.print_usage.assert_not_called()
    assert write_line.call_args_list == [
        call(sys.stderr, line) for line in NO_ARGUMENTS_LINES
    ]


def test_usage_rendering_requires_the_parser_that_owns_the_usage_text() -> None:
    usage = models.CliError(models.ExitCode.USAGE, "public usage")
    with pytest.raises(
        RuntimeError,
        match=r"^a usage error requires its argument parser$",
    ):
        cli._render_cli_error(usage, json_requested=False)


def test_interrupt_and_expected_error_preserve_structured_inputs() -> None:
    with patch.object(cli, "_write_json_error") as write_json:
        assert cli._write_interrupt(json_requested=True) == int(
            models.ExitCode.INTERRUPTED
        )
    error = write_json.call_args.args[0]
    assert (error.code, error.message) == (
        models.ExitCode.INTERRUPTED,
        "interrupted",
    )

    parser = MagicMock(spec=models.ArgumentParser)
    parser.parse_args.side_effect = models.CliError(
        models.ExitCode.INPUT_ERROR,
        "public input",
    )
    with (
        patch.object(cli, "_build_parser", return_value=parser),
        patch.object(cli, "_write_cli_error", return_value=3) as write_error,
    ):
        assert cli.main(["public.eml"]) == 3
    write_error.assert_called_once_with(
        parser.parse_args.side_effect,
        json_requested=False,
        parser=parser,
        no_arguments=False,
    )

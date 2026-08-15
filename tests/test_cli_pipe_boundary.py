"""Contracts for closed command-line output streams."""

from __future__ import annotations

import os
import sys
from unittest.mock import patch

import pytest

from eml_attachment_remover import cli, cli_boundary, models


@pytest.mark.parametrize("output_format", ["human", "json", "paths"])
def test_broken_stdout_pipe_returns_quiet_stable_status(output_format: str) -> None:
    with (
        patch.object(cli, "_run_batch", side_effect=BrokenPipeError),
        patch.object(cli, "handle_broken_pipe", return_value=1) as handle,
    ):
        status = cli.main(["--output-format", output_format, "public.eml"])

    assert status == 1
    handle.assert_called_once_with()


def test_failed_error_stream_does_not_escape_the_cli_boundary() -> None:
    with (
        patch.object(cli, "_run_batch", side_effect=RuntimeError("PUBLIC INTERNAL")),
        patch.object(cli, "_write_error", side_effect=OSError("PUBLIC STDERR CLOSED")),
    ):
        status = cli.main(["public.eml"])

    assert status == int(models.ExitCode.INTERNAL_ERROR)


@pytest.mark.parametrize(
    ("failure", "expected"),
    [
        (BrokenPipeError(), 1),
        (OSError("PUBLIC STDERR CLOSED"), int(models.ExitCode.INTERNAL_ERROR)),
    ],
)
def test_no_argument_guidance_preserves_the_stream_failure_boundary(
    failure: OSError,
    expected: int,
) -> None:
    """Keep the first-run screen inside the established quiet-I/O boundary."""
    with (
        patch.object(cli, "_write_line", side_effect=failure),
        patch.object(cli, "handle_broken_pipe", return_value=1),
    ):
        status = cli.main([])

    assert status == expected


@pytest.mark.parametrize(
    ("failure", "output_format", "expected"),
    [
        (models.CliError(models.ExitCode.INPUT_ERROR, "PUBLIC"), "human", 1),
        (models.CliError(models.ExitCode.INPUT_ERROR, "PUBLIC"), "json", 1),
        (KeyboardInterrupt(), "human", 1),
        (KeyboardInterrupt(), "json", 1),
    ],
)
def test_expected_error_reporting_never_escapes_failed_streams(
    failure: BaseException,
    output_format: str,
    expected: int,
) -> None:
    if output_format == "json":
        writer = "_write_json_error"
    elif isinstance(failure, KeyboardInterrupt):
        writer = "_write_line"
    else:
        writer = "_write_error"
    with (
        patch.object(cli, "_run_batch", side_effect=failure),
        patch.object(cli, writer, side_effect=BrokenPipeError),
        patch.object(cli, "handle_broken_pipe", return_value=1),
        patch.object(cli, "quiet_error_status", return_value=70),
    ):
        status = cli.main(["--output-format", output_format, "public.eml"])

    assert status == expected


@pytest.mark.parametrize("output_format", ["human", "json"])
def test_interrupt_reporting_io_error_returns_internal_status(
    output_format: str,
) -> None:
    writer = "_write_json_error" if output_format == "json" else "_write_line"
    with (
        patch.object(cli, "_run_batch", side_effect=KeyboardInterrupt),
        patch.object(cli, writer, side_effect=OSError("PUBLIC STREAM CLOSED")),
    ):
        status = cli.main(["--output-format", output_format, "public.eml"])

    assert status == int(models.ExitCode.INTERNAL_ERROR)


def test_broken_pipe_handler_redirects_stdout_and_closes_its_descriptor() -> None:
    with (
        patch.object(os, "open", return_value=91) as open_null,
        patch.object(os, "dup2") as duplicate,
        patch.object(os, "close") as close,
        patch.object(sys.stdout, "fileno", return_value=17),
    ):
        status = cli_boundary.handle_broken_pipe()

    assert status == cli_boundary.BROKEN_PIPE_STATUS
    open_null.assert_called_once_with(os.devnull, os.O_WRONLY)
    duplicate.assert_called_once_with(91, 17)
    close.assert_called_once_with(91)


def test_broken_pipe_handler_tolerates_redirection_failure() -> None:
    with patch.object(os, "open", side_effect=OSError("PUBLIC CLOSED")):
        assert cli_boundary.handle_broken_pipe() == cli_boundary.BROKEN_PIPE_STATUS

    assert cli_boundary.quiet_error_status() == int(models.ExitCode.INTERNAL_ERROR)


def test_broken_pipe_handler_closes_null_descriptor_when_duplication_fails() -> None:
    with (
        patch.object(os, "open", return_value=91),
        patch.object(os, "dup2", side_effect=OSError("PUBLIC DUP FAILURE")),
        patch.object(os, "close") as close,
        patch.object(sys.stdout, "fileno", return_value=17),
    ):
        assert cli_boundary.handle_broken_pipe() == cli_boundary.BROKEN_PIPE_STATUS

    close.assert_called_once_with(91)

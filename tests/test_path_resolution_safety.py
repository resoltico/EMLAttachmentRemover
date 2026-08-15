# ruff: file-ignore[private-member-access]
"""Contracts for failures while canonically binding output parents."""

from __future__ import annotations

import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest

from eml_attachment_remover import cli, models, paths


def test_output_parent_resolution_failure_is_a_write_error() -> None:
    with (
        patch.object(Path, "resolve", side_effect=RuntimeError("PUBLIC LOOP")),
        pytest.raises(models.CliError) as raised,
    ):
        paths._canonical_destination(Path.cwd() / "public.eml")

    assert raised.value.code is models.ExitCode.WRITE_ERROR


def test_output_directory_resolution_failure_is_a_write_error() -> None:
    with tempfile.TemporaryDirectory() as directory:
        with (
            patch.object(Path, "resolve", side_effect=RuntimeError("PUBLIC LOOP")),
            pytest.raises(models.CliError) as raised,
        ):
            paths._validate_output_directory(Path(directory))

    assert raised.value.code is models.ExitCode.WRITE_ERROR


def test_skip_existing_late_inspection_failure_is_a_write_error() -> None:
    source = Path("public.eml")
    destination = Path("public-output.eml")
    with (
        patch.object(cli, "_destination_exists", return_value=True),
        patch.object(Path, "lstat", side_effect=OSError("PUBLIC RACE")),
        pytest.raises(models.CliError) as raised,
    ):
        cli._existing_destination_outcome(
            source,
            destination,
            dry_run=False,
            skip_existing=True,
        )

    assert raised.value.code is models.ExitCode.WRITE_ERROR

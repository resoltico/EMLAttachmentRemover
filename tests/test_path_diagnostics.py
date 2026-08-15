# ruff: file-ignore[private-member-access]
"""Exact public diagnostics and fail-closed path-resolution contracts."""

from __future__ import annotations

import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest

from eml_attachment_remover import models, paths


def test_missing_source_diagnostic_names_the_requested_path() -> None:
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "missing.eml"
        with pytest.raises(models.CliError) as raised:
            paths._validate_source(source)

    assert raised.value.code is models.ExitCode.INPUT_ERROR
    assert raised.value.message == f"input file does not exist: {source}"


def test_missing_destination_parent_diagnostic_names_the_parent() -> None:
    with tempfile.TemporaryDirectory() as directory:
        parent = Path(directory) / "missing"
        with pytest.raises(models.CliError) as raised:
            paths._validate_destination_parent(parent / "output.eml")

    assert raised.value.code is models.ExitCode.WRITE_ERROR
    assert raised.value.message == f"output directory does not exist: {parent}"


def test_shared_output_directory_resolution_is_strict() -> None:
    output_directory = Path("public-output-directory")
    directory_metadata = type("Metadata", (), {"st_mode": 0o040700})()
    resolved = Path("/public/resolved-output-directory")
    with (
        patch.object(paths, "_absolute_path", return_value=output_directory),
        patch.object(Path, "stat", return_value=directory_metadata),
        patch.object(Path, "resolve", return_value=resolved) as resolve,
    ):
        assert paths._validate_output_directory(output_directory) == resolved

    resolve.assert_called_once_with(strict=True)


def test_batch_output_alias_diagnostic_names_the_selected_source() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        source = root / "message.eml"
        selected_output = root / "message.attachments-removed.eml"
        source.write_bytes(b"PUBLIC SOURCE")
        selected_output.write_bytes(b"PUBLIC SELECTED OUTPUT")
        canonical_output = selected_output.parent.resolve() / selected_output.name

        with pytest.raises(models.CliError) as raised:
            paths._planned_outputs([source, selected_output], None, None)

    assert raised.value.code is models.ExitCode.OUTPUT_CONFLICT
    assert raised.value.message == (
        f"refusing to use a selected source file as a batch output: {canonical_output}"
    )

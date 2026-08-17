# ruff: file-ignore[private-member-access]
"""Regression tests for final CLI and filesystem safety boundaries."""

from __future__ import annotations

import json
import os
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

import pytest

from eml_attachment_remover import cli, models, paths
from tests.test_support import simple_message


def _message_file(path: Path) -> None:
    """Write one public message with a removable attachment."""
    path.write_bytes(simple_message().as_bytes())


def test_absolute_path_normalizes_dot_segments_without_following_final_link() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        target = base / "target.eml"
        link = base / "link.eml"
        target.write_bytes(b"PUBLIC")
        try:
            link.symlink_to(target)
        except OSError:
            link = target

        normalized = paths._absolute_path(base / "child" / ".." / link.name)

        assert normalized == link
        assert normalized != target.resolve() or link == target


def test_dot_segment_duplicate_outputs_are_rejected_before_execution() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        _message_file(source)
        dotted = base / "child" / ".." / "source.eml"

        with pytest.raises(models.CliError) as raised:
            paths._planned_outputs([source, dotted], None, None)

    assert raised.value.code is models.ExitCode.OUTPUT_CONFLICT


def test_symbolic_parent_duplicate_outputs_are_rejected_when_supported() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        real = base / "real"
        alias = base / "alias"
        real.mkdir()
        try:
            alias.symlink_to(real, target_is_directory=True)
        except OSError:
            with (
                patch.object(
                    paths,
                    "_output_collision_path",
                    return_value=real / "same.text-only.eml",
                ),
                pytest.raises(models.CliError),
            ):
                paths._planned_outputs(
                    [real / "same.eml", base / "same.eml"],
                    None,
                    real,
                )
            return
        first = real / "same.eml"
        second = alias / "same.eml"
        _message_file(first)

        with pytest.raises(models.CliError) as raised:
            paths._planned_outputs([first, second], None, None)

    assert raised.value.code is models.ExitCode.OUTPUT_CONFLICT


def test_dry_run_ignores_write_only_output_paths_and_batch_collisions() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        first_parent = base / "one"
        second_parent = base / "two"
        first_parent.mkdir()
        second_parent.mkdir()
        first = first_parent / "same.eml"
        second = second_parent / "same.eml"
        _message_file(first)
        _message_file(second)
        missing = base / "missing"

        status = cli.main([
            "--dry-run",
            "--output-dir",
            str(missing),
            str(first),
            str(second),
        ])

        assert status == 0
        assert not missing.exists()


def test_json_scanner_obeys_end_of_options_and_the_last_explicit_value() -> None:
    scanner = cli._json_output_requested

    assert not scanner(["--", "--output-format=json"])
    assert not scanner(["--output-format=json", "--output-format", "human"])
    assert scanner(["--output-format", "human", "--output-format=json"])
    assert not scanner([
        "--output-format",
        "--",
        "--output-format",
        "json",
    ])


def test_parser_rejects_ambiguous_long_option_abbreviations() -> None:
    parser = cli._build_parser()

    with pytest.raises(models.CliError) as raised:
        parser.parse_args(["--output-f", "json", "public.eml"])

    assert "unrecognized arguments" in raised.value.message


def test_json_internal_and_interrupt_failures_remain_json_documents() -> None:
    failures = (
        (RuntimeError("PUBLIC INTERNAL"), models.ExitCode.INTERNAL_ERROR),
        (KeyboardInterrupt(), models.ExitCode.INTERRUPTED),
    )
    for failure, expected_code in failures:
        stdout = StringIO()
        stderr = StringIO()
        with (
            patch.object(cli, "_run_batch", side_effect=failure),
            redirect_stdout(stdout),
            redirect_stderr(stderr),
        ):
            status = cli.main(["--output-format=json", "public.eml"])

        document = json.loads(stdout.getvalue())
        assert status == int(expected_code)
        assert document["errors"][0]["error"]["code"] == int(expected_code)
        assert not stderr.getvalue()


def test_skip_inspection_error_is_source_qualified_batch_failure() -> None:
    source = Path.cwd() / "public.eml"
    destination = Path.cwd() / "public-output.eml"
    with (
        patch.object(cli, "_validate_source", return_value=source),
        patch.object(
            cli,
            "_destination_exists",
            side_effect=models.CliError(
                models.ExitCode.WRITE_ERROR,
                "PUBLIC BLOCKED",
            ),
        ),
    ):
        outcome = cli._execute_plans(
            [(source, destination)],
            dry_run=False,
            force=False,
            skip_existing=True,
            fail_fast=False,
        )

    assert outcome.failures == (
        models.BatchFailure(source, models.ExitCode.WRITE_ERROR, "PUBLIC BLOCKED"),
    )


@pytest.mark.parametrize("source_kind", ["missing", "directory"])
def test_skip_existing_never_conceals_an_invalid_source(source_kind: str) -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        if source_kind == "directory":
            source.mkdir()
        destination = base / "source.text-only.eml"
        destination.write_bytes(b"PUBLIC EXISTING")

        outcome = cli._execute_plans(
            [(source, destination)],
            dry_run=False,
            force=False,
            skip_existing=True,
            fail_fast=False,
        )

    assert len(outcome.failures) == 1
    assert outcome.failures[0].code is models.ExitCode.INPUT_ERROR
    assert not outcome.skips


def test_skip_existing_preserves_source_inspection_failure() -> None:
    source = Path.cwd() / "public.eml"
    destination = Path.cwd() / "public-output.eml"
    with patch.object(
        cli,
        "_validate_source",
        side_effect=models.CliError(models.ExitCode.INPUT_ERROR, "PUBLIC BLOCKED"),
    ):
        outcome = cli._execute_plans(
            [(source, destination)],
            dry_run=False,
            force=False,
            skip_existing=True,
            fail_fast=False,
        )

    assert outcome.failures == (
        models.BatchFailure(source, models.ExitCode.INPUT_ERROR, "PUBLIC BLOCKED"),
    )


@pytest.mark.parametrize("target_kind", ["ordinary", "source"])
def test_skip_existing_rejects_symbolic_output(target_kind: str) -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        target = source if target_kind == "source" else base / "target.eml"
        destination = base / "destination.eml"
        _message_file(source)
        if target_kind != "source":
            target.write_bytes(b"PUBLIC TARGET")
        try:
            destination.symlink_to(target)
        except OSError:
            with (
                patch.object(Path, "lstat") as lstat,
                patch.object(cli, "_destination_exists", return_value=True),
            ):
                lstat.return_value.st_mode = 0o120777
                outcome = cli._execute_plans(
                    [(source, destination)],
                    dry_run=False,
                    force=False,
                    skip_existing=True,
                    fail_fast=False,
                )
        else:
            outcome = cli._execute_plans(
                [(source, destination)],
                dry_run=False,
                force=False,
                skip_existing=True,
                fail_fast=False,
            )

    assert len(outcome.failures) == 1
    assert outcome.failures[0].code is models.ExitCode.OUTPUT_CONFLICT


def test_skip_existing_rejects_a_late_hard_link_alias() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        destination = base / "destination.eml"
        _message_file(source)
        try:
            os.link(source, destination)
        except OSError:
            destination.write_bytes(b"PUBLIC")
            alias_patch = patch.object(paths, "_paths_alias", return_value=True)
        else:
            alias_patch = patch.object(paths, "_paths_alias", wraps=paths._paths_alias)
        with alias_patch:
            outcome = cli._execute_plans(
                [(source, destination)],
                dry_run=False,
                force=False,
                skip_existing=True,
                fail_fast=False,
            )

    assert len(outcome.failures) == 1
    assert outcome.failures[0].code is models.ExitCode.OUTPUT_CONFLICT


def test_skip_existing_rejects_fifo_when_supported() -> None:
    if not hasattr(os, "mkfifo"):
        return
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        destination = base / "destination.eml"
        _message_file(source)
        os.mkfifo(destination)

        outcome = cli._execute_plans(
            [(source, destination)],
            dry_run=False,
            force=False,
            skip_existing=True,
            fail_fast=False,
        )

    assert len(outcome.failures) == 1
    assert outcome.failures[0].code is models.ExitCode.OUTPUT_CONFLICT


def test_plans_use_normalized_absolute_sources_and_destinations() -> None:
    source = Path("public") / ".." / "public.eml"
    output = Path("public") / ".." / "output.eml"

    with patch.object(paths, "_validate_source", side_effect=lambda path: path):
        assert paths._planned_outputs([source], output, None) == [
            (Path.cwd() / "public.eml", Path.cwd().resolve() / "output.eml"),
        ]


def test_destination_parent_symlink_is_bound_to_canonical_directory() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        real = base / "real"
        alias = base / "alias"
        real.mkdir()
        _message_file(source)
        try:
            alias.symlink_to(real, target_is_directory=True)
        except OSError:
            with patch.object(
                Path,
                "resolve",
                return_value=real,
            ):
                _source, destination = paths._validate_paths(
                    source,
                    alias / "output.eml",
                    force=False,
                )
        else:
            _source, destination = paths._validate_paths(
                source,
                alias / "output.eml",
                force=False,
            )

    assert destination == real.resolve() / "output.eml"


def test_output_directory_is_returned_as_canonical_path() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        real = base / "real"
        alias = base / "alias"
        real.mkdir()
        try:
            alias.symlink_to(real, target_is_directory=True)
        except OSError:
            with patch.object(Path, "resolve", return_value=real):
                validated = paths._validate_output_directory(alias)
        else:
            validated = paths._validate_output_directory(alias)

    assert validated == real.resolve()


def test_planned_output_parent_resolution_failure_is_a_write_error() -> None:
    with (
        patch.object(
            paths,
            "_output_collision_path",
            side_effect=OSError("PUBLIC INSPECTION BLOCKED"),
        ),
        patch.object(paths, "_validate_source", side_effect=lambda path: path),
    ):
        with pytest.raises(models.CliError) as raised:
            paths._planned_outputs([Path("public.eml")], None, None)

    assert raised.value.code is models.ExitCode.WRITE_ERROR
    assert "PUBLIC INSPECTION BLOCKED" in raised.value.message

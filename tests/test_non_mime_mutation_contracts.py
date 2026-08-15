# ruff: file-ignore[no-self-use, private-member-access]
"""Direct contracts for non-MIME mutation-sensitive application boundaries."""

from __future__ import annotations

import argparse
import ctypes
import errno
import os
import stat
import sys
import tempfile
import unittest
from email.message import EmailMessage
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, call, patch

import pytest

from eml_attachment_remover import (
    atomic_publish,
    cli,
    cli_boundary,
    models,
    paths,
    processing,
    storage,
)


def _native_library(result: int = 0) -> tuple[MagicMock, MagicMock]:
    """Return a synthetic C library and its shared rename operation.

    Returns:
        A library exposing both supported rename symbols and their callable.

    """
    operation = MagicMock(return_value=result)
    library = MagicMock()
    library.renamex_np = operation
    library.renameat2 = operation
    return library, operation


def _output_plan(base: Path, *, modified: bool = False) -> models.OutputPlan:
    """Return a public synthetic storage plan.

    Returns:
        A complete plan with real source, destination, and temporary context.

    """
    source = base / "source.eml"
    source.write_bytes(b"PUBLIC SOURCE")
    message = EmailMessage()
    message.set_content("Public body")
    return models.OutputPlan(
        source,
        base / "destination.eml",
        message,
        b"PUBLIC SOURCE",
        modified=modified,
        force=False,
    )


class TestNonMimeMutationContracts(unittest.TestCase):
    """Require exact externally meaningful data at application boundaries."""

    def test_native_publish_declares_the_exact_c_abi(self) -> None:
        cases = (
            ("darwin", [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]),
            (
                "linux",
                [
                    ctypes.c_int,
                    ctypes.c_char_p,
                    ctypes.c_int,
                    ctypes.c_char_p,
                    ctypes.c_uint,
                ],
            ),
        )
        for platform_name, expected_types in cases:
            with self.subTest(platform=platform_name):
                library, operation = _native_library()
                with (
                    patch.object(sys, "platform", platform_name),
                    patch.object(ctypes, "CDLL", return_value=library),
                ):
                    assert atomic_publish._native_no_replace(
                        Path("temporary.eml"),
                        Path("destination.eml"),
                    )

                assert operation.argtypes == expected_types
                assert operation.restype is ctypes.c_int

    def test_native_publish_preserves_os_error_identity(self) -> None:
        library, _operation = _native_library(-1)
        destination = Path("destination.eml")
        with (
            patch.object(sys, "platform", "darwin"),
            patch.object(ctypes, "CDLL", return_value=library),
            patch.object(ctypes, "get_errno", return_value=errno.EIO),
            pytest.raises(OSError, match=r"destination\.eml") as raised,
        ):
            atomic_publish._native_no_replace(Path("temporary.eml"), destination)

        assert raised.value.errno == errno.EIO
        assert raised.value.strerror == os.strerror(errno.EIO)
        assert raised.value.filename == destination

    def test_existing_destination_outcomes_preserve_exact_context(self) -> None:
        source = Path("source.eml")
        destination = Path("destination.eml")
        with (
            patch.object(cli, "_destination_exists", return_value=True),
            patch.object(Path, "lstat", side_effect=OSError("inspection blocked")),
            pytest.raises(models.CliError) as inspection,
        ):
            cli._existing_destination_outcome(
                source,
                destination,
                dry_run=False,
                skip_existing=True,
            )
        assert (inspection.value.code, inspection.value.message) == (
            models.ExitCode.WRITE_ERROR,
            (
                "could not inspect existing output path destination.eml: "
                "inspection blocked"
            ),
        )

        metadata = SimpleNamespace(st_mode=stat.S_IFREG | stat.S_IRUSR)
        with (
            patch.object(cli, "_destination_exists", return_value=True),
            patch.object(Path, "lstat", return_value=metadata),
            patch.object(cli, "_paths_alias", return_value=True),
        ):
            alias = cli._existing_destination_outcome(
                source,
                destination,
                dry_run=False,
                skip_existing=True,
            )
        assert alias == models.BatchFailure(
            source,
            models.ExitCode.OUTPUT_CONFLICT,
            "existing output aliases the source EML: destination.eml",
        )

        with (
            patch.object(cli, "_destination_exists", return_value=True),
            patch.object(Path, "lstat", return_value=metadata),
            patch.object(cli, "_paths_alias", return_value=False),
            patch.object(
                cli, "_absolute_path", return_value=Path("/public/output.eml")
            ),
        ):
            skipped = cli._existing_destination_outcome(
                source,
                destination,
                dry_run=False,
                skip_existing=True,
            )
        assert skipped == models.BatchSkip(source, Path("/public/output.eml"))

    def test_outcome_reporting_forwards_exact_mode_flags(self) -> None:
        outcome = models.BatchOutcome((), (), ())
        with (
            patch.object(cli, "_write_paths") as write_paths,
            patch.object(cli, "_write_failures") as write_failures,
        ):
            cli._report_outcome(outcome, models.OutputFormat.PATHS, multiple=True)
        write_paths.assert_called_once_with([], [], nul_terminated=False)
        write_failures.assert_called_once_with(())

        with (
            patch.object(cli, "_write_paths") as write_paths,
            patch.object(cli, "_write_failures") as write_failures,
        ):
            cli._report_outcome(outcome, models.OutputFormat.PATHS0, multiple=True)
        write_paths.assert_called_once_with([], [], nul_terminated=True)
        write_failures.assert_called_once_with(())

        with patch.object(cli, "_write_human_batch") as write_human:
            cli._report_outcome(outcome, models.OutputFormat.HUMAN, multiple=True)
        write_human.assert_called_once_with([], [], [], multiple=True)

    def test_batch_orchestration_preserves_every_namespace_value(self) -> None:
        for source_count, multiple in ((1, False), (2, True)):
            with self.subTest(source_count=source_count):
                sources = [Path(f"public-{index}.eml") for index in range(source_count)]
                arguments = argparse.Namespace(
                    source=sources,
                    output=None,
                    output_dir=None,
                    output_format=models.OutputFormat.JSON,
                    dry_run=True,
                    force=True,
                    skip_existing=True,
                    fail_fast=True,
                )
                outcome = models.BatchOutcome((), (), ())
                with (
                    patch.object(cli, "_absolute_path", side_effect=lambda path: path),
                    patch.object(
                        cli,
                        "_default_destination",
                        side_effect=lambda path: path.with_suffix(".out"),
                    ),
                    patch.object(cli, "_validate_cli_arguments") as validate,
                    patch.object(
                        cli, "_execute_plans", return_value=outcome
                    ) as execute,
                    patch.object(cli, "_report_outcome") as report,
                    patch.object(
                        cli, "_outcome_exit_code", return_value=17
                    ) as exit_code,
                ):
                    assert cli._run_batch(arguments) == 17

                validate.assert_called_once_with(
                    sources,
                    None,
                    models.OutputFormat.JSON,
                    dry_run=True,
                )
                execute.assert_called_once_with(
                    [(source, source.with_suffix(".out")) for source in sources],
                    dry_run=True,
                    force=True,
                    skip_existing=True,
                    fail_fast=True,
                )
                report.assert_called_once_with(
                    outcome,
                    models.OutputFormat.JSON,
                    multiple=multiple,
                )
                exit_code.assert_called_once_with(
                    outcome,
                    source_count=source_count,
                    fail_fast=True,
                )

    def test_broken_pipe_cleanup_suppresses_close_failure(self) -> None:
        with (
            patch.object(os, "open", return_value=91),
            patch.object(os, "dup2"),
            patch.object(os, "close", side_effect=OSError("close blocked")) as close,
            patch.object(sys.stdout, "fileno", return_value=17),
        ):
            assert cli_boundary.handle_broken_pipe() == cli_boundary.BROKEN_PIPE_STATUS
        close.assert_called_once_with(91)

    def test_path_resolution_is_strict_and_failures_are_exact(self) -> None:
        destination = Path("parent") / "output.eml"
        resolved = Path("/public/parent")
        with (
            patch.object(paths, "_validate_destination_parent"),
            patch.object(Path, "resolve", return_value=resolved) as resolve,
        ):
            assert paths._canonical_destination(destination) == resolved / "output.eml"
        resolve.assert_called_once_with(strict=True)

        with (
            patch.object(paths, "_validate_destination_parent"),
            patch.object(Path, "resolve", side_effect=OSError("resolve blocked")),
            pytest.raises(models.CliError) as raised,
        ):
            paths._canonical_destination(destination)
        assert raised.value.message == (
            "could not resolve output directory parent: resolve blocked"
        )

        directory = Path("public-directory")
        metadata = SimpleNamespace(st_mode=stat.S_IFDIR | stat.S_IRUSR)
        with (
            patch.object(paths, "_absolute_path", return_value=directory),
            patch.object(Path, "stat", return_value=metadata),
            patch.object(Path, "resolve", side_effect=OSError("resolve blocked")),
            pytest.raises(models.CliError) as output_error,
        ):
            paths._validate_output_directory(directory)
        assert output_error.value.message == (
            "could not resolve output directory public-directory: resolve blocked"
        )

    def test_path_alias_and_collision_fallbacks_are_mandatory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"PUBLIC")
            with patch.object(paths, "_paths_alias", return_value=False):
                assert paths._paths_alias_if_present(source, source)

        first = Path("first.eml")
        second = Path("second.eml")
        with (
            patch.object(paths, "_absolute_path", side_effect=lambda path: path),
            patch.object(paths, "_validate_single_batch"),
            patch.object(
                paths,
                "_output_collision_path",
                side_effect=(Path("first.out"), Path("second.out")),
            ),
            patch.object(paths, "_path_collision_key", side_effect=("first", "second")),
            patch.object(paths, "_paths_alias", return_value=True) as alias,
            patch.object(paths, "_paths_alias_if_present", return_value=False),
            pytest.raises(models.CliError) as collision,
        ):
            paths._planned_outputs([first, second], None, None)
        assert alias.call_args == call(Path("first.out"), Path("second.out"))
        assert collision.value.message == (
            "multiple inputs resolve to the same output path: "
            "first.out and second.attachments-removed.eml"
        )

    def test_source_reader_uses_exact_portable_flags_and_source(self) -> None:
        source = Path("public.eml")
        with (
            patch.object(os, "O_RDONLY", 4),
            patch.object(os, "O_BINARY", 8, create=True),
            patch.object(os, "O_NONBLOCK", 16, create=True),
            patch.object(os, "open", side_effect=OSError("open blocked")) as open_,
            pytest.raises(models.CliError),
        ):
            processing._read_source(source)
        open_.assert_called_once_with(source, 28)

        input_file = MagicMock()
        input_file.__enter__.return_value = input_file
        input_file.read.return_value = b"PUBLIC"
        with (
            patch.object(os, "open", return_value=91),
            patch.object(os, "fdopen", return_value=input_file),
            patch.object(processing, "_require_regular_source") as require_regular,
        ):
            assert processing._read_source(source) == b"PUBLIC"
        require_regular.assert_called_once_with(input_file, source)

    def test_removal_warnings_preserve_separator_and_parent_guard(self) -> None:
        part = EmailMessage()
        removed = [models.RemovedPart((0,), "application/pdf", "public.pdf", None)]

        warnings = processing._removal_warnings(
            (((0,), None, part),),
            {"application/pkcs7-mime", "multipart/signed"},
            (),
            removed,
        )

        assert warnings == [
            (
                "protected MIME entity left intact: application/pkcs7-mime, "
                "multipart/signed"
            )
        ]

    def test_storage_forwards_structure_and_reports_cleanup_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            plan = _output_plan(base, modified=True)
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"PUBLIC")
            structure = (("public",),)
            with (
                patch.object(storage, "_leaf_fingerprints", return_value=()),
                patch.object(storage, "_structure_fingerprint", return_value=structure),
                patch.object(storage, "_write_temporary", return_value=temporary),
                patch.object(storage, "_verify_serialized_message") as verify,
                patch.object(storage, "_apply_source_mode", return_value=None),
                patch.object(storage, "_temporary_size", return_value=6),
                patch.object(storage, "_commit_output"),
            ):
                assert storage._produce_output(plan) == (6, None)
            verify.assert_called_once_with(temporary, (), structure)

            temporary.write_bytes(b"PUBLIC")
            with (
                patch.object(storage, "_leaf_fingerprints", return_value=()),
                patch.object(storage, "_structure_fingerprint", return_value=structure),
                patch.object(storage, "_write_temporary", return_value=temporary),
                patch.object(storage, "_verify_serialized_message"),
                patch.object(storage, "_apply_source_mode", return_value=None),
                patch.object(storage, "_temporary_size", return_value=6),
                patch.object(storage, "_commit_output", side_effect=OSError("blocked")),
                patch.object(Path, "unlink", side_effect=OSError("cleanup blocked")),
                pytest.raises(models.CliError) as cleanup,
            ):
                storage._produce_output(plan)
        assert cleanup.value.message.endswith(
            "(output was not published): cleanup blocked"
        )

    def test_unchanged_verification_failure_message_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"PUBLIC CHANGED")
            with pytest.raises(models.CliError) as raised:
                storage._verify_unchanged_output(temporary, b"PUBLIC EXPECTED")

        assert (raised.value.code, raised.value.message) == (
            models.ExitCode.VERIFICATION_ERROR,
            "unchanged output did not preserve the source bytes exactly",
        )

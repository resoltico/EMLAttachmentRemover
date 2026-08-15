"""Verify private ephemeral Hypothesis storage orchestration."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import hypothesis_publication, hypothesis_run

if TYPE_CHECKING:
    from collections.abc import Callable


class HypothesisRunTests(unittest.TestCase):
    """Exercise isolation, publication, cleanup, and failure preservation."""

    def test_nonobservable_run_uses_and_removes_private_external_storage(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            seen: list[Path] = []

            def action(storage: Path) -> None:
                seen.append(storage)
                self.assertFalse(storage.is_relative_to(project))
                constants = storage / "constants"
                constants.mkdir()
                (constants / "private").write_text(str(Path.home()), encoding="utf-8")

            hypothesis_run.run_isolated(action, project)
        self.assertEqual(len(seen), 1)
        self.assertFalse(seen[0].parent.exists())

    def test_observable_run_publishes_only_sanitized_jsonl_and_keeps_examples(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            examples = project / ".hypothesis" / "examples"
            examples.mkdir(parents=True)
            replay = examples / "public-replay"
            replay.write_bytes(b"opaque")
            seen: list[Path] = []

            def action(storage: Path) -> None:
                seen.append(storage)
                observed = storage / "observed"
                observed.mkdir()
                record = {
                    "coverage": {str(storage / "private.py"): [1]},
                    "metadata": {"os.getpid()": 9, "public": "retained"},
                    "value": str(project / "tests" / "public.py"),
                }
                (observed / "2026-01-01_testcases.jsonl").write_text(
                    f"{json.dumps(record)}\n",
                    encoding="utf-8",
                )
                (storage / "constants").mkdir()
                (storage / "patches").mkdir()

            hypothesis_run.run_isolated(action, project, observations=True)
            published = project / ".hypothesis" / "observed"
            content = next(published.iterdir()).read_text(encoding="utf-8")
            record = json.loads(content)
            self.assertTrue(replay.is_file())
        self.assertFalse(seen[0].parent.exists())
        self.assertNotIn(str(project), content)
        self.assertNotIn(str(seen[0]), content)
        self.assertIsNone(record["coverage"])
        self.assertEqual(record["metadata"], {"public": "retained"})

    def test_action_failures_and_interrupts_are_preserved_after_cleanup(self) -> None:
        def failing_action(
            error: BaseException, seen_paths: list[Path]
        ) -> Callable[[Path], None]:
            def fail(storage: Path) -> None:
                seen_paths.append(storage)
                raise error

            return fail

        for failure in (RuntimeError("test failed"), KeyboardInterrupt()):
            seen: list[Path] = []
            with self.subTest(exception=type(failure).__name__):
                with self.assertRaises(type(failure)) as raised:
                    hypothesis_run.run_isolated(
                        failing_action(failure, seen),
                        Path.cwd(),
                    )
                self.assertIs(raised.exception, failure)
                self.assertFalse(seen[0].parent.exists())

    def test_action_publication_and_cleanup_failures_are_grouped(self) -> None:
        action_error = RuntimeError("test failed")
        publication_error = ValueError("publication failed")
        cleanup_error = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def fail(_storage: Path) -> None:
            raise action_error

        def clean_then_fail(root: Path) -> None:
            real_cleanup(root)
            raise cleanup_error

        with (
            patch.object(
                hypothesis_run.publisher,
                "publish_observations",
                side_effect=publication_error,
            ),
            patch.object(hypothesis_run, "_cleanup", side_effect=clean_then_fail),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(fail, Path.cwd(), observations=True)
        self.assertEqual(
            raised.exception.exceptions,
            (action_error, publication_error, cleanup_error),
        )

    def test_junit_report_is_published_even_when_the_action_fails(self) -> None:
        action_error = RuntimeError("test failed")
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            destination = project / "build" / "test-results.xml"
            seen: list[Path] = []

            def fail(storage: Path) -> None:
                seen.append(storage)
                (storage.parent / "test-results.xml").write_text(
                    "<testsuites><testsuite/></testsuites>",
                    encoding="utf-8",
                )
                raise action_error

            with self.assertRaises(RuntimeError) as raised:
                hypothesis_run.run_isolated(
                    fail,
                    project,
                    report_destination=destination,
                )
            self.assertIs(raised.exception, action_error)
            self.assertTrue(destination.is_file())
            self.assertFalse(seen[0].parent.exists())

    def test_action_and_junit_publication_failures_are_grouped(self) -> None:
        action_error = RuntimeError("test failed")
        publication_error = ValueError("report publication failed")

        def fail(_storage: Path) -> None:
            raise action_error

        with (
            patch.object(
                hypothesis_run.junit_report,
                "publish",
                side_effect=publication_error,
            ),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(
                fail,
                Path.cwd(),
                report_destination=Path("public-report.xml"),
            )
        self.assertEqual(raised.exception.exceptions, (action_error, publication_error))

    def test_interrupt_and_finalization_failure_form_base_exception_group(self) -> None:
        interrupt = KeyboardInterrupt()
        finalization_error = ValueError("finalization failed")

        def fail(_storage: Path) -> None:
            raise interrupt

        with (
            patch.object(
                hypothesis_run.artifacts,
                "finalize",
                side_effect=finalization_error,
            ),
            self.assertRaises(BaseExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(fail, Path.cwd())
        self.assertEqual(raised.exception.exceptions, (interrupt, finalization_error))

    def test_finalization_and_cleanup_failures_without_action_are_reported(
        self,
    ) -> None:
        cleanup_error = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def clean_then_fail(root: Path) -> None:
            real_cleanup(root)
            raise cleanup_error

        with (
            patch.object(
                hypothesis_run.artifacts,
                "finalize",
                side_effect=ValueError("finalization failed"),
            ),
            patch.object(hypothesis_run, "_cleanup", side_effect=clean_then_fail),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(lambda _storage: None, Path.cwd())
        self.assertEqual(len(raised.exception.exceptions), 2)
        with (
            patch.object(
                hypothesis_run.artifacts,
                "finalize",
                side_effect=ValueError("finalization failed"),
            ),
            self.assertRaisesRegex(ValueError, "finalization failed"),
        ):
            hypothesis_run.run_isolated(lambda _storage: None, Path.cwd())

    def test_unsafe_temp_location_is_rejected_without_deleting_unowned_path(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            unsafe = project / "unsafe-storage"
            unsafe.mkdir()
            with (
                patch.object(tempfile, "mkdtemp", return_value=str(unsafe)),
                self.assertRaisesRegex(
                    hypothesis_run.HypothesisRunError,
                    "temporary storage is unsafe",
                ),
            ):
                hypothesis_run.run_isolated(lambda _storage: None, project)
            self.assertTrue(unsafe.is_dir())

    def test_project_ancestor_can_never_be_treated_as_owned_temporary_storage(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            ancestor = Path(directory).resolve()
            project = ancestor / "project"
            project.mkdir()
            with (
                patch.object(tempfile, "mkdtemp", return_value=str(ancestor)),
                self.assertRaisesRegex(
                    hypothesis_run.HypothesisRunError,
                    "temporary storage is unsafe",
                ),
            ):
                hypothesis_run.run_isolated(lambda _storage: None, project)
            self.assertTrue(project.is_dir())

    def test_failed_initialization_removes_only_the_owned_temporary_root(self) -> None:
        owned = Path(tempfile.mkdtemp(prefix=hypothesis_run.STORAGE_PREFIX)).resolve()
        with (
            patch.object(tempfile, "mkdtemp", return_value=str(owned)),
            patch.object(
                hypothesis_run,
                "_initialize_storage",
                side_effect=ValueError("initialization failed"),
            ),
            self.assertRaisesRegex(ValueError, "initialization failed"),
        ):
            hypothesis_run._create_storage(  # ruff: ignore[private-member-access]
                Path.cwd()
            )
        self.assertFalse(owned.exists())

    def test_cleanup_reports_operational_and_incomplete_removal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "storage"
            root.mkdir()
            with (
                patch(
                    "tools.hypothesis_run.shutil.rmtree",
                    side_effect=OSError("denied"),
                ),
                self.assertRaisesRegex(
                    hypothesis_run.HypothesisRunError,
                    "cannot remove",
                ),
            ):
                hypothesis_run._cleanup(root)  # ruff: ignore[private-member-access]
            with (
                patch("tools.hypothesis_run.shutil.rmtree"),
                self.assertRaisesRegex(
                    hypothesis_run.HypothesisRunError,
                    "cleanup was incomplete",
                ),
            ):
                hypothesis_run._cleanup(root)  # ruff: ignore[private-member-access]


class HypothesisPublicationTests(unittest.TestCase):
    """Exercise fail-closed public observation replacement."""

    def test_directory_contract_rejects_unreadable_nonregular_and_link_entries(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            regular = root / "regular"
            regular.write_text("public", encoding="utf-8")
            link = root / "link"
            link.symlink_to(regular)
            for path in (regular, link):
                with (
                    self.subTest(path=path.name),
                    self.assertRaisesRegex(
                        hypothesis_publication.HypothesisPublicationError,
                        "must be a real directory",
                    ),
                ):
                    hypothesis_publication._directory_exists(  # ruff: ignore[private-member-access]
                        path, "public test entry"
                    )
            with (
                patch.object(Path, "lstat", side_effect=OSError("denied")),
                self.assertRaisesRegex(
                    hypothesis_publication.HypothesisPublicationError,
                    "cannot inspect public test entry",
                ),
            ):
                hypothesis_publication._directory_exists(  # ruff: ignore[private-member-access]
                    root, "public test entry"
                )

    def test_observable_run_must_emit_at_least_one_file(self) -> None:
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
            self.assertRaisesRegex(
                hypothesis_publication.HypothesisPublicationError,
                "produced no observation files",
            ),
        ):
            hypothesis_publication.publish_observations(
                Path(source_directory), Path(publication_directory)
            )

    def test_public_root_creation_failure_is_contextual(self) -> None:
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
            patch.object(
                hypothesis_publication.artifacts,
                "finalize",
                return_value=(1, 1),
            ),
            patch.object(Path, "mkdir", side_effect=OSError("denied")),
            self.assertRaisesRegex(
                hypothesis_publication.HypothesisPublicationError,
                "cannot create public Hypothesis root",
            ),
        ):
            hypothesis_publication.publish_observations(
                Path(source_directory), Path(publication_directory)
            )

    def test_existing_publication_is_replaced_only_after_sanitization(self) -> None:
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source = Path(source_directory).resolve()
            publication = Path(publication_directory).resolve()
            observed = source / ".hypothesis" / "observed"
            observed.mkdir(parents=True)
            (observed / "public.jsonl").write_text(
                '{"status":"passed"}\n', encoding="utf-8"
            )
            destination = publication / ".hypothesis" / "observed"
            destination.mkdir(parents=True)
            stale = destination / "stale.jsonl"
            stale.write_text("{}\n", encoding="utf-8")
            self.assertEqual(
                hypothesis_publication.publish_observations(source, publication),
                (1, 1),
            )
            self.assertFalse(stale.exists())
            self.assertEqual(
                (destination / "public.jsonl").read_text(encoding="utf-8"),
                '{"status":"passed"}\n',
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)

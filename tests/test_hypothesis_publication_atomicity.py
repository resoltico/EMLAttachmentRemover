"""Verify transactional publication of sanitized Hypothesis observations."""

from __future__ import annotations

import shutil
import tempfile
import unittest
from pathlib import Path
from typing import cast
from unittest.mock import patch

from tools import hypothesis_publication


def _observation_roots(
    source_directory: str,
    publication_directory: str,
) -> tuple[Path, Path, Path]:
    """Return populated source, publication, and destination paths.

    Returns:
        The two roots and existing public observation directory.

    """
    source = Path(source_directory).resolve()
    publication = Path(publication_directory).resolve()
    observed = source / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    (observed / "public.jsonl").write_text(
        '{"status":"passed"}\n',
        encoding="utf-8",
    )
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "stale.jsonl").write_text("{}\n", encoding="utf-8")
    return source, publication, destination


class HypothesisPublicationAtomicityTests(unittest.TestCase):
    """Exercise staging, promotion rollback, and backup retention."""

    def test_staging_creation_failure_preserves_prior_set(self) -> None:
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(tempfile, "mkdtemp", side_effect=OSError("denied")),
                self.assertRaisesRegex(
                    hypothesis_publication.HypothesisPublicationError,
                    "cannot create Hypothesis observation staging directory",
                ),
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertTrue((destination / "stale.jsonl").is_file())

    def test_unexpected_copy_interrupt_cleans_staging_and_preserves_prior_set(
        self,
    ) -> None:
        interrupt = KeyboardInterrupt()
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(shutil, "copytree", side_effect=interrupt),
                self.assertRaises(KeyboardInterrupt) as raised,
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertIs(raised.exception, interrupt)
            self.assertTrue((destination / "stale.jsonl").is_file())
            self.assertEqual(
                list(
                    destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
                ),
                [],
            )

    def test_partial_copy_failure_preserves_prior_set_and_removes_staging(self) -> None:
        def fail_after_partial_copy(
            _source: Path,
            destination: Path,
            *,
            copy_function: object,
        ) -> None:
            del copy_function
            Path(destination).mkdir()
            (Path(destination) / "partial.jsonl").write_text(
                "partial\n",
                encoding="utf-8",
            )
            message = "copy failed after partial output"
            raise OSError(message)

        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(shutil, "copytree", side_effect=fail_after_partial_copy),
                self.assertRaisesRegex(
                    hypothesis_publication.HypothesisPublicationError,
                    "cannot stage public Hypothesis observations",
                ),
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertEqual(
                [path.name for path in destination.iterdir()],
                ["stale.jsonl"],
            )
            self.assertEqual(
                list(
                    destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
                ),
                [],
            )

    def test_promotion_failure_restores_prior_set_and_removes_staging(self) -> None:
        real_replace = Path.replace

        def fail_staged_promotion(path: Path, target: Path) -> Path:
            if (
                path.name == hypothesis_publication.STAGING_OBSERVATIONS_NAME
                and path.parent.name.startswith(hypothesis_publication.STAGING_PREFIX)
            ):
                message = "promotion denied"
                raise OSError(message)
            return real_replace(path, target)

        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(
                    Path, "replace", autospec=True, side_effect=fail_staged_promotion
                ),
                self.assertRaisesRegex(
                    hypothesis_publication.HypothesisPublicationError,
                    "cannot promote Hypothesis observations",
                ),
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertTrue((destination / "stale.jsonl").is_file())
            self.assertEqual(
                list(
                    destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
                ),
                [],
            )

    def test_promotion_failure_without_prior_set_removes_staging(self) -> None:
        real_replace = Path.replace

        def fail_staged_promotion(path: Path, target: Path) -> Path:
            if path.name == hypothesis_publication.STAGING_OBSERVATIONS_NAME:
                message = "promotion denied"
                raise OSError(message)
            return real_replace(path, target)

        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            (destination / "stale.jsonl").unlink()
            destination.rmdir()
            with (
                patch.object(
                    Path, "replace", autospec=True, side_effect=fail_staged_promotion
                ),
                self.assertRaisesRegex(
                    hypothesis_publication.HypothesisPublicationError,
                    "cannot promote Hypothesis observations",
                ),
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertFalse(destination.exists())
            self.assertEqual(
                list(
                    destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
                ),
                [],
            )

    def test_restore_failure_retains_the_only_prior_set_in_staging(self) -> None:
        real_replace = Path.replace

        def fail_promotion_and_restore(path: Path, target: Path) -> Path:
            if path.parent.name.startswith(hypothesis_publication.STAGING_PREFIX) and (
                path.name
                in {
                    hypothesis_publication.STAGING_OBSERVATIONS_NAME,
                    hypothesis_publication.PREVIOUS_OBSERVATIONS_NAME,
                }
            ):
                message = "promotion or restoration denied"
                raise OSError(message)
            return real_replace(path, target)

        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(
                    Path,
                    "replace",
                    autospec=True,
                    side_effect=fail_promotion_and_restore,
                ),
                self.assertRaises(ExceptionGroup) as raised,
            ):
                hypothesis_publication.publish_observations(source, publication)

            staging = list(
                destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
            )
            self.assertEqual(len(staging), 1)
            backup = staging[0] / hypothesis_publication.PREVIOUS_OBSERVATIONS_NAME
            self.assertTrue((backup / "stale.jsonl").is_file())
            self.assertFalse(destination.exists())
            restore_error = raised.exception.exceptions[1]
            self.assertEqual(
                str(restore_error),
                "cannot restore prior Hypothesis observations; retained in staging "
                f"entry {staging[0].name!r}/'previous'",
            )
            self.assertNotIn(str(publication), str(restore_error))

    def test_control_flow_restore_failure_retains_prior_set_in_staging(self) -> None:
        real_replace = Path.replace
        interrupt = KeyboardInterrupt()

        def interrupt_promotion_and_fail_restore(path: Path, target: Path) -> Path:
            if path.parent.name.startswith(hypothesis_publication.STAGING_PREFIX):
                if path.name == hypothesis_publication.STAGING_OBSERVATIONS_NAME:
                    raise interrupt
                if path.name == hypothesis_publication.PREVIOUS_OBSERVATIONS_NAME:
                    message = "restoration denied"
                    raise OSError(message)
            return real_replace(path, target)

        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source, publication, destination = _observation_roots(
                source_directory,
                publication_directory,
            )
            with (
                patch.object(
                    Path,
                    "replace",
                    autospec=True,
                    side_effect=interrupt_promotion_and_fail_restore,
                ),
                self.assertRaises(BaseExceptionGroup) as raised,
            ):
                hypothesis_publication.publish_observations(source, publication)

            self.assertIs(raised.exception.exceptions[0], interrupt)
            staging = list(
                destination.parent.glob(f"{hypothesis_publication.STAGING_PREFIX}*")
            )
            self.assertEqual(len(staging), 1)
            backup = staging[0] / hypothesis_publication.PREVIOUS_OBSERVATIONS_NAME
            self.assertTrue((backup / "stale.jsonl").is_file())

    def test_staging_cleanup_failures_preserve_prior_failures(self) -> None:
        cases: tuple[
            tuple[BaseException | None, type[BaseException]],
            ...,
        ] = (
            (None, hypothesis_publication.HypothesisPublicationError),
            (ValueError("promotion failed"), ExceptionGroup),
            (KeyboardInterrupt(), BaseExceptionGroup),
        )
        for action_error, expected_type in cases:
            with (
                self.subTest(action=type(action_error).__name__),
                tempfile.TemporaryDirectory() as directory,
            ):
                staging = Path(directory) / "staging"
                staging.mkdir()
                with (
                    patch.object(shutil, "rmtree", side_effect=OSError("denied")),
                    self.assertRaises(expected_type) as raised,
                ):
                    hypothesis_publication._discard_staging(  # ruff: ignore[private-member-access]
                        staging,
                        action_error,
                    )

                if action_error is not None:
                    self.assertIsInstance(raised.exception, BaseExceptionGroup)
                    group = cast("BaseExceptionGroup[BaseException]", raised.exception)
                    self.assertIs(group.exceptions[0], action_error)
                else:
                    self.assertEqual(
                        str(raised.exception),
                        "cannot remove Hypothesis observation staging directory "
                        f"{staging}: denied",
                    )

    def test_restore_is_not_attempted_over_an_existing_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            previous = root / hypothesis_publication.PREVIOUS_OBSERVATIONS_NAME
            destination = root / hypothesis_publication.OBSERVATION_DIRECTORY
            previous.mkdir()
            destination.mkdir()
            with patch.object(Path, "replace", autospec=True) as replace:
                hypothesis_publication._restore_previous(  # ruff: ignore[private-member-access]
                    previous,
                    destination,
                    ValueError("promotion failed"),
                )
        self.assertEqual(replace.call_count, 0)

    def test_staging_copy_and_cleanup_failures_preserve_both_causes(self) -> None:
        cases: tuple[tuple[BaseException, type[BaseExceptionGroup]], ...] = (
            (OSError("copy failed"), ExceptionGroup),
            (KeyboardInterrupt(), BaseExceptionGroup),
        )
        for copy_error, group_type in cases:
            with (
                self.subTest(error=type(copy_error).__name__),
                tempfile.TemporaryDirectory() as directory,
                patch.object(shutil, "copytree", side_effect=copy_error),
                patch.object(shutil, "rmtree", side_effect=OSError("cleanup failed")),
                self.assertRaises(group_type) as raised,
            ):
                hypothesis_publication._stage_observations(  # ruff: ignore[private-member-access]
                    Path(directory) / "source",
                    Path(directory),
                )

            group = raised.exception
            self.assertEqual(len(group.exceptions), 2)
            if isinstance(copy_error, Exception):
                self.assertIsInstance(
                    group.exceptions[0],
                    hypothesis_publication.HypothesisPublicationError,
                )
                self.assertIn("cannot stage public", str(group.exceptions[0]))
            else:
                self.assertIs(group.exceptions[0], copy_error)
            self.assertEqual(
                str(group.exceptions[1]).split(": ")[-1],
                "cleanup failed",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)

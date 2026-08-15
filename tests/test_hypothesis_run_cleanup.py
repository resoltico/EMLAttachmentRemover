"""Verify cleanup for unexpected Hypothesis finalization failures."""

from __future__ import annotations

import unittest
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import hypothesis_run

if TYPE_CHECKING:
    from collections.abc import Callable


def _record_storage(seen: list[Path]) -> Callable[[Path], None]:
    """Return an action that records the isolated Hypothesis storage.

    Returns:
        The storage-recording action.

    """

    def record(storage: Path) -> None:
        seen.append(storage)

    return record


class HypothesisUnexpectedFailureCleanupTests(unittest.TestCase):
    """Exercise unconditional cleanup and exact failure preservation."""

    def test_unexpected_observation_publication_failures_still_clean_storage(
        self,
    ) -> None:
        for failure in (
            TypeError("publication contract failed"),
            KeyboardInterrupt(),
            SystemExit(7),
        ):
            seen: list[Path] = []
            with (
                self.subTest(exception=type(failure).__name__),
                patch.object(
                    hypothesis_run.publisher,
                    "publish_observations",
                    side_effect=failure,
                ),
                self.assertRaises(type(failure)) as raised,
            ):
                hypothesis_run.run_isolated(
                    _record_storage(seen),
                    Path.cwd(),
                    observations=True,
                )

            self.assertIs(raised.exception, failure)
            self.assertEqual(len(seen), 1)
            self.assertFalse(seen[0].parent.exists())

    def test_unexpected_report_publication_failures_still_clean_storage(
        self,
    ) -> None:
        for failure in (
            TypeError("report contract failed"),
            KeyboardInterrupt(),
            SystemExit(8),
        ):
            seen: list[Path] = []
            with (
                self.subTest(exception=type(failure).__name__),
                patch.object(
                    hypothesis_run.junit_report,
                    "publish",
                    side_effect=failure,
                ),
                self.assertRaises(type(failure)) as raised,
            ):
                hypothesis_run.run_isolated(
                    _record_storage(seen),
                    Path.cwd(),
                    report_destination=Path("public-report.xml"),
                )

            self.assertIs(raised.exception, failure)
            self.assertEqual(len(seen), 1)
            self.assertFalse(seen[0].parent.exists())

    def test_unexpected_publication_and_cleanup_failures_are_grouped(self) -> None:
        publication_error = TypeError("publication contract failed")
        cleanup_error = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

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
            hypothesis_run.run_isolated(
                lambda _storage: None,
                Path.cwd(),
                observations=True,
            )

        self.assertEqual(
            raised.exception.exceptions,
            (publication_error, cleanup_error),
        )

    def test_action_interrupt_publication_and_cleanup_failures_form_base_group(
        self,
    ) -> None:
        action_error = RuntimeError("test action failed")
        publication_error = KeyboardInterrupt()
        cleanup_error = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def fail_action(_storage: Path) -> None:
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
            self.assertRaises(BaseExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(
                fail_action,
                Path.cwd(),
                observations=True,
            )

        self.assertEqual(
            raised.exception.exceptions,
            (action_error, publication_error, cleanup_error),
        )

    def test_two_expected_finalization_failures_are_grouped_before_cleanup(
        self,
    ) -> None:
        report_error = ValueError("report publication failed")
        artifact_error = RuntimeError("artifact finalization failed")
        seen: list[Path] = []
        with (
            patch.object(
                hypothesis_run.junit_report,
                "publish",
                side_effect=report_error,
            ),
            patch.object(
                hypothesis_run.artifacts,
                "finalize",
                side_effect=artifact_error,
            ),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(
                _record_storage(seen),
                Path.cwd(),
                report_destination=Path("public-report.xml"),
            )

        self.assertEqual(raised.exception.exceptions, (report_error, artifact_error))
        self.assertFalse(seen[0].parent.exists())

    def test_action_and_cleanup_failures_survive_successful_finalization(self) -> None:
        action_error = RuntimeError("test action failed")
        cleanup_error = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def fail_action(_storage: Path) -> None:
            raise action_error

        def clean_then_fail(root: Path) -> None:
            real_cleanup(root)
            raise cleanup_error

        with (
            patch.object(hypothesis_run, "_cleanup", side_effect=clean_then_fail),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(fail_action, Path.cwd())

        self.assertEqual(raised.exception.exceptions, (action_error, cleanup_error))

    def test_cleanup_failure_without_prior_failure_is_preserved(self) -> None:
        cleanup_error = TypeError("unexpected cleanup failure")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def clean_then_fail(root: Path) -> None:
            real_cleanup(root)
            raise cleanup_error

        with (
            patch.object(hypothesis_run, "_cleanup", side_effect=clean_then_fail),
            self.assertRaises(TypeError) as raised,
        ):
            hypothesis_run.run_isolated(lambda _storage: None, Path.cwd())

        self.assertIs(raised.exception, cleanup_error)


if __name__ == "__main__":
    unittest.main(verbosity=2)

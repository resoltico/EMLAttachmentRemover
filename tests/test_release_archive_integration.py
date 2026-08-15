"""Focused release archive, staging-audit, and cleanup orchestration tests."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

from tools import qualify_release


class ReleaseArchiveIntegrationTests(unittest.TestCase):
    """Exercise fresh audit filtering and verifier error translation."""

    def test_staging_audit_ignores_only_its_exact_current_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            staging = root / ".release-dist.public"
            staging.mkdir()
            inside = MagicMock(path=staging, message="staging output")
            audit = MagicMock(issues=(inside,), public_files=())
            with (
                patch.object(qualify_release, "PROJECT_ROOT", root),
                patch.object(
                    qualify_release.check_repository_hygiene,
                    "audit_repository",
                    return_value=audit,
                ),
                patch.object(
                    qualify_release.verify_distribution_archives,
                    "verify_distribution_archives",
                ) as verify,
            ):
                qualify_release._verify_distribution_files(  # ruff: ignore[private-member-access]
                    staging,
                    (
                        "public-1-cp314-none-any.whl",
                        "public-1.tar.gz",
                        "remove-eml-attachments.pyz",
                    ),
                    allow_staging=True,
                )
            self.assertTrue(staging.is_dir())
            verify.assert_called_once()

    def test_archive_verification_rejects_other_or_nonstaging_audit_issues(
        self,
    ) -> None:
        names = (
            "public-1-cp314-none-any.whl",
            "public-1.tar.gz",
            "remove-eml-attachments.pyz",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            staging = root / ".release-dist.public"
            staging.mkdir()
            outside = root / "unexpected.txt"
            issue = MagicMock(path=outside, message="unexpected public path")
            audit = MagicMock(issues=(issue,), public_files=())
            for allow_staging in (False, True):
                with (
                    self.subTest(allow_staging=allow_staging),
                    patch.object(qualify_release, "PROJECT_ROOT", root),
                    patch.object(
                        qualify_release.check_repository_hygiene,
                        "audit_repository",
                        return_value=audit,
                    ),
                    self.assertRaisesRegex(
                        qualify_release.ReleaseQualificationError,
                        "unexpected.txt: unexpected public path",
                    ),
                ):
                    qualify_release._verify_distribution_files(  # ruff: ignore[private-member-access]
                        staging,
                        names,
                        allow_staging=allow_staging,
                    )

    def test_nonstaging_audit_reports_every_candidate_issue_with_exact_lines(
        self,
    ) -> None:
        names = (
            "public-1-cp314-none-any.whl",
            "public-1.tar.gz",
            "remove-eml-attachments.pyz",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            candidate = root / "candidate"
            candidate.mkdir()
            issues = (
                MagicMock(path=candidate / "alpha.txt", message="first issue"),
                MagicMock(path=candidate / "beta.txt", message="second issue"),
            )
            audit = MagicMock(issues=issues, public_files=())
            with (
                patch.object(qualify_release, "PROJECT_ROOT", root),
                patch.object(
                    qualify_release.check_repository_hygiene,
                    "audit_repository",
                    return_value=audit,
                ),
                self.assertRaises(qualify_release.ReleaseQualificationError) as raised,
            ):
                qualify_release._verify_distribution_files(  # ruff: ignore[private-member-access]
                    candidate,
                    names,
                    allow_staging=False,
                )
        self.assertEqual(
            str(raised.exception),
            "repository changed during release build:\n"
            "candidate/alpha.txt: first issue\n"
            "candidate/beta.txt: second issue",
        )

    def test_archive_verifier_receives_every_exact_audited_input(self) -> None:
        names = (
            "public-1-cp314-none-any.whl",
            "public-1.tar.gz",
            "remove-eml-attachments.pyz",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            candidate = root / "downloaded-candidate"
            project_config = root / "pyproject.toml"
            public_files = (root / "README.md", root / "src/public.py")
            audit = MagicMock(issues=(), public_files=public_files)
            with (
                patch.object(qualify_release, "PROJECT_ROOT", root),
                patch.object(qualify_release, "PROJECT_CONFIG", project_config),
                patch.object(
                    qualify_release.check_repository_hygiene,
                    "audit_repository",
                    return_value=audit,
                ) as audit_repository,
                patch.object(
                    qualify_release.verify_distribution_archives,
                    "verify_distribution_archives",
                ) as verify,
            ):
                qualify_release._verify_distribution_files(  # ruff: ignore[private-member-access]
                    candidate,
                    names,
                    allow_staging=False,
                )
        self.assertEqual(audit_repository.call_args, call(root))
        self.assertEqual(
            verify.call_args,
            call(
                candidate / "public-1.tar.gz",
                candidate / "public-1-cp314-none-any.whl",
                root,
                project_config,
                public_files,
            ),
        )

    def test_archive_verifier_failure_is_public_release_failure(self) -> None:
        audit = MagicMock(issues=(), public_files=())
        failure = qualify_release.verify_distribution_archives.DistributionArchiveError(
            "unsafe public archive",
        )
        with (
            patch.object(
                qualify_release.check_repository_hygiene,
                "audit_repository",
                return_value=audit,
            ),
            patch.object(
                qualify_release.verify_distribution_archives,
                "verify_distribution_archives",
                side_effect=failure,
            ),
            self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "unsafe public archive",
            ),
        ):
            qualify_release._verify_distribution_files(  # ruff: ignore[private-member-access]
                Path("/public"),
                (
                    "public-1-cp314-none-any.whl",
                    "public-1.tar.gz",
                    "remove-eml-attachments.pyz",
                ),
                allow_staging=False,
            )


class ReleaseCleanupTests(unittest.TestCase):
    """Exercise explicit staging-cleanup failure aggregation."""

    def test_cleanup_operational_failure_is_aggregated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release-dist"
            with (
                patch.object(
                    qualify_release,
                    "_complete_staging",
                    side_effect=RuntimeError("qualification failed"),
                ),
                patch(
                    "tools.qualify_release.shutil.rmtree",
                    side_effect=OSError("cleanup denied"),
                ),
                self.assertRaises(BaseExceptionGroup) as raised,
            ):
                qualify_release.qualify_release(output)
        self.assertEqual(
            raised.exception.message,
            "release qualification failed and staging cleanup also failed",
        )
        self.assertIn("cleanup denied", str(raised.exception.exceptions[1]))

    def test_cleanup_incomplete_removal_is_aggregated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release-dist"
            with (
                patch.object(
                    qualify_release,
                    "_complete_staging",
                    side_effect=RuntimeError("qualification failed"),
                ),
                patch("tools.qualify_release.shutil.rmtree"),
                self.assertRaises(BaseExceptionGroup) as raised,
            ):
                qualify_release.qualify_release(output)
        self.assertIn("cleanup was incomplete", str(raised.exception.exceptions[1]))


if __name__ == "__main__":
    unittest.main(verbosity=2)

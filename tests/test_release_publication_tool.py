"""Behavioral tests for release publication and CLI output."""

from __future__ import annotations

import io
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import qualify_release


class ReleasePublicationTests(unittest.TestCase):
    """Exercise atomic publication, failure cleanup, and command-line output."""

    def test_qualification_atomically_publishes_only_four_validated_files(self) -> None:
        names = qualify_release._artifact_names(
            "public-project",
            "9.8.7",
        )
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release-dist"

            def build(staging: Path, artifact_names: tuple[str, str, str]) -> None:
                self.assertEqual(artifact_names, names)
                for name in artifact_names:
                    (staging / name).write_bytes(f"public {name}".encode())

            with (
                patch.object(
                    qualify_release,
                    "_project_identity",
                    return_value=("public-project", "9.8.7"),
                ),
                patch.object(qualify_release, "_build_and_test", side_effect=build),
            ):
                published = qualify_release.qualify_release(output)
            self.assertEqual(
                {path.name for path in published},
                {*names, qualify_release.CHECKSUM_FILE_NAME},
            )
            self.assertEqual(
                {path.name for path in output.iterdir()},
                {*names, qualify_release.CHECKSUM_FILE_NAME},
            )
            manifest = (output / qualify_release.CHECKSUM_FILE_NAME).read_text(
                encoding="utf-8",
            )
            self.assertNotIn(str(output), manifest)

    def test_qualification_removes_staging_after_any_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output = base / "release-dist"
            with (
                patch.object(
                    qualify_release,
                    "_project_identity",
                    return_value=("public-project", "9.8.7"),
                ),
                patch.object(
                    qualify_release,
                    "_complete_staging",
                    side_effect=subprocess.CalledProcessError(7, ("public",)),
                ),
                self.assertRaises(subprocess.CalledProcessError),
            ):
                qualify_release.qualify_release(output)
            self.assertFalse(output.exists())
            self.assertEqual(list(base.glob(".release-dist.*")), [])

    def test_qualification_failure_preserves_initial_empty_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output = base / "release-dist"
            output.mkdir()
            with (
                patch.object(
                    qualify_release,
                    "_project_identity",
                    return_value=("public-project", "9.8.7"),
                ),
                patch.object(
                    qualify_release,
                    "_complete_staging",
                    side_effect=RuntimeError("public failure"),
                ),
                self.assertRaisesRegex(RuntimeError, "public failure"),
            ):
                qualify_release.qualify_release(output)
            self.assertTrue(output.is_dir())
            self.assertEqual(tuple(output.iterdir()), ())

    def test_public_qualification_rejects_symlink_without_touching_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "target"
            target.mkdir()
            (target / "public.txt").write_text("must remain", encoding="utf-8")
            symbolic = base / "release-dist"
            symbolic.symlink_to(target, target_is_directory=True)
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "absent or empty directory",
            ):
                qualify_release.qualify_release(symbolic)
            self.assertEqual(
                (target / "public.txt").read_text(encoding="utf-8"),
                "must remain",
            )

    def test_publish_failure_restores_an_empty_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output = base / "release-dist"
            output.mkdir()
            staging = base / ".release-dist.public"
            staging.mkdir()
            with (
                patch.object(Path, "replace", side_effect=OSError("publish failed")),
                self.assertRaisesRegex(OSError, "publish failed"),
            ):
                qualify_release._publish_staging(staging, output)
            self.assertTrue(output.is_dir())

    def test_publish_rejects_changed_destination_and_absent_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output = base / "release-dist"
            output.mkdir()
            (output / "public.txt").write_text("changed", encoding="utf-8")
            staging = base / ".release-dist.public"
            staging.mkdir()
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "must remain absent or empty",
            ):
                qualify_release._publish_staging(staging, output)

        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output = base / "release-dist"
            staging = base / ".release-dist.public"
            staging.mkdir()
            with (
                patch.object(Path, "replace", side_effect=OSError("publish failed")),
                self.assertRaisesRegex(OSError, "publish failed"),
            ):
                qualify_release._publish_staging(staging, output)
            self.assertFalse(output.exists())

    def test_publish_rejects_a_symbolic_destination_even_when_it_is_empty(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "target"
            target.mkdir()
            output = base / "release-dist"
            output.symlink_to(target, target_is_directory=True)
            staging = base / ".release-dist.public"
            staging.mkdir()
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "must remain absent or empty",
            ):
                qualify_release._publish_staging(staging, output)
            self.assertTrue(output.is_symlink())
            self.assertTrue(staging.is_dir())

    def test_main_prints_every_qualified_path(self) -> None:
        paths = (Path("/public/alpha"), Path("/public/zeta"))
        output = Path("/public/release-dist")
        with (
            patch.object(qualify_release, "qualify_release", return_value=paths) as run,
            patch("sys.stdout", new_callable=io.StringIO) as stdout,
        ):
            status = qualify_release.main(["--output-directory", str(output)])
        self.assertEqual(status, 0)
        self.assertEqual(stdout.getvalue(), "/public/alpha\n/public/zeta\n")
        run.assert_called_once_with(output)


if __name__ == "__main__":
    unittest.main(verbosity=2)

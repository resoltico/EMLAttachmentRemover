"""Behavioral tests for safe zipapp publication and CLI dispatch."""

from __future__ import annotations

import hashlib
import io
import os
import stat
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from tools import build_zipapp


class ZipappPublicationTests(unittest.TestCase):
    """Exercise atomic publication, cleanup, and command-line dispatch."""

    def test_output_validation_normalizes_paths_and_rejects_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "public.pyz"
            checksum = base / "SHA256SUMS"
            normalized_target, normalized_checksum = build_zipapp._validate_outputs(
                target,
                checksum,
            )
            expected_target = target.parent.resolve() / target.name
            expected_checksum = checksum.parent.resolve() / checksum.name
            self.assertEqual(normalized_target, expected_target)
            self.assertEqual(normalized_checksum, expected_checksum)
            without_checksum = build_zipapp._validate_outputs(target, None)
            self.assertEqual(without_checksum, (expected_target, None))
            with self.assertRaisesRegex(ValueError, "distinct files"):
                build_zipapp._validate_outputs(target, target)

            target.write_bytes(b"public archive")
            os.link(target, checksum)
            with self.assertRaisesRegex(ValueError, "distinct files"):
                build_zipapp._validate_outputs(target, checksum)

    def test_output_validation_preserves_exact_labels_and_alias_diagnostic(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "public.pyz"
            checksum = base / "SHA256SUMS"
            for path, label in ((target, "zipapp"), (checksum, "checksum")):
                referent = base / f"{label}.txt"
                referent.write_text("public", encoding="utf-8")
                path.symlink_to(referent)
                with self.assertRaises(ValueError) as raised:
                    build_zipapp._validate_outputs(target, checksum)
                self.assertEqual(
                    str(raised.exception),
                    f"{label} output must not be a symbolic link: "
                    f"{path.parent.resolve() / path.name}",
                )
                path.unlink()
            with self.assertRaises(ValueError) as raised:
                build_zipapp._validate_outputs(target, target)
            self.assertEqual(
                str(raised.exception),
                "zipapp and checksum outputs must be distinct files",
            )

    def test_nested_output_parents_and_checksum_use_canonical_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "archive.pyz"
            archive.write_bytes(b"archive")
            checksum = root / "nested" / "reports" / "SHA256SUMS"
            original_write = Path.write_bytes
            payloads: list[bytes] = []

            def recording_write(path: Path, data: bytes) -> int:
                payloads.append(data)
                return original_write(path, data)

            with patch.object(Path, "write_bytes", recording_write):
                build_zipapp._write_checksum(checksum, archive)
            self.assertEqual(len(payloads), 1)
            self.assertTrue(payloads[0].endswith(b"  archive.pyz\n"))
            self.assertTrue(checksum.is_file())

    def test_output_validation_rejects_symlinks_without_changing_referents(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            referent = base / "referent.bin"
            referent.write_bytes(b"must remain")
            for label in ("target", "checksum"):
                symbolic = base / label
                symbolic.symlink_to(referent)
                with self.subTest(label=label):
                    self._assert_symbolic_output_rejected(base, symbolic, label)
            self.assertEqual(referent.read_bytes(), b"must remain")

    def _assert_symbolic_output_rejected(
        self,
        base: Path,
        symbolic: Path,
        label: str,
    ) -> None:
        with self.assertRaisesRegex(ValueError, "symbolic link"):
            build_zipapp._validate_outputs(
                symbolic if label == "target" else base / "public.pyz",
                symbolic if label == "checksum" else None,
            )

    def test_same_output_handles_missing_distinct_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            self.assertFalse(
                build_zipapp._same_output(
                    base / "alpha",
                    base / "zeta",
                ),
            )

    def test_checksum_is_atomic_and_uses_only_the_archive_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            archive = base / "public.pyz"
            checksum = base / "nested" / "SHA256SUMS"
            archive.write_bytes(b"public archive")
            build_zipapp._write_checksum(checksum, archive)
            expected = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(
                checksum.read_text(encoding="utf-8"),
                f"{expected}  public.pyz\n",
            )

    def test_checksum_failure_removes_the_reserved_temporary_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            archive = base / "public.pyz"
            checksum = base / "SHA256SUMS"
            archive.write_bytes(b"public archive")
            with (
                patch.object(Path, "replace", side_effect=OSError("public failure")),
                self.assertRaisesRegex(OSError, "public failure"),
            ):
                build_zipapp._write_checksum(checksum, archive)
            self.assertEqual(list(base.glob(".SHA256SUMS.*.tmp")), [])
            self.assertFalse(checksum.exists())

    def test_build_zipapp_publishes_an_executable_verified_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "nested" / "public.pyz"
            built = build_zipapp.build_zipapp(target, verify=False)
            mode = built.stat().st_mode
            self.assertEqual(built, target.resolve())
            self.assertTrue(zipfile.is_zipfile(built))
            self.assertTrue(mode & stat.S_IXUSR)
            self.assertTrue(mode & stat.S_IXGRP)
            self.assertTrue(mode & stat.S_IXOTH)

    def test_build_failure_removes_the_reserved_temporary_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "public.pyz"
            temporary = base / ".public.pyz.public.tmp"
            temporary.write_bytes(b"")
            with (
                patch.object(build_zipapp, "_temporary_path", return_value=temporary),
                patch.object(
                    build_zipapp,
                    "_write_archive",
                    side_effect=RuntimeError("public failure"),
                ),
                self.assertRaisesRegex(RuntimeError, "public failure"),
            ):
                build_zipapp.build_zipapp(target, verify=False)
            self.assertFalse(temporary.exists())
            self.assertFalse(target.exists())

    def test_permission_failure_preserves_existing_target_and_removes_temporary(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "public.pyz"
            target.write_bytes(b"previous public archive")
            temporary = base / ".public.pyz.public.tmp"
            temporary.write_bytes(b"")
            with (
                patch.object(build_zipapp, "_temporary_path", return_value=temporary),
                patch.object(build_zipapp, "_write_archive"),
                patch.object(build_zipapp, "_verify_archive"),
                patch.object(Path, "chmod", side_effect=OSError("permission denied")),
                self.assertRaisesRegex(OSError, "permission denied"),
            ):
                build_zipapp.build_zipapp(target, verify=False)
            self.assertEqual(target.read_bytes(), b"previous public archive")
            self.assertFalse(temporary.exists())

    def test_main_builds_with_requested_flags_and_checksum(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "public.pyz"
            checksum = Path(directory) / "SHA256SUMS"
            with (
                patch.object(
                    build_zipapp, "build_zipapp", return_value=target
                ) as build,
                patch.object(build_zipapp, "_write_checksum") as write_checksum,
                patch("sys.stdout", new_callable=io.StringIO) as stdout,
            ):
                status = build_zipapp.main([
                    "--target",
                    str(target),
                    "--checksum-file",
                    str(checksum),
                ])
            self.assertEqual(status, 0)
            self.assertEqual(stdout.getvalue(), f"{target}\n")
            normalized_target = target.parent.resolve() / target.name
            normalized_checksum = checksum.parent.resolve() / checksum.name
            build.assert_called_once_with(normalized_target, verify=True)
            write_checksum.assert_called_once_with(normalized_checksum, target)

    def test_main_can_disable_verification_without_a_checksum(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "public.pyz"
            with (
                patch.object(
                    build_zipapp, "build_zipapp", return_value=target
                ) as build,
                patch.object(build_zipapp, "_write_checksum") as write_checksum,
                patch("sys.stdout", new_callable=io.StringIO),
            ):
                status = build_zipapp.main([
                    "--target",
                    str(target),
                    "--no-verify",
                ])
            self.assertEqual(status, 0)
            build.assert_called_once_with(
                target.parent.resolve() / target.name,
                verify=False,
            )
            write_checksum.assert_not_called()


if __name__ == "__main__":
    unittest.main(verbosity=2)

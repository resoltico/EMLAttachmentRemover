"""Behavioral tests for portable release-candidate qualification."""

from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import qualify_release


class ReleaseIdentityTests(unittest.TestCase):
    """Exercise canonical metadata and exact artifact-name derivation."""

    def test_project_identity_reads_canonical_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "pyproject.toml"
            config.write_text(
                '[project]\nname = "public.project-name"\nversion = "9.8.7"\n',
                encoding="utf-8",
            )
            with patch.object(qualify_release, "PROJECT_CONFIG", config):
                identity = qualify_release._project_identity()  # ruff: ignore[private-member-access]
        self.assertEqual(identity, ("public.project-name", "9.8.7"))

    def test_artifact_names_are_normalized_versioned_and_sorted(self) -> None:
        names = qualify_release._artifact_names(  # ruff: ignore[private-member-access]
            "public.project-name",
            "9.8.7",
        )
        self.assertEqual(names, tuple(sorted(names)))
        self.assertEqual(
            set(names),
            {
                "public_project_name-9.8.7-cp314-none-any.whl",
                "public_project_name-9.8.7.tar.gz",
                "remove-eml-attachments.pyz",
            },
        )


class ReleaseDirectoryTests(unittest.TestCase):
    """Exercise safe staging and exact regular-file allowlisting."""

    def test_prepare_stages_beside_an_absent_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "nested" / "release-dist"
            staging = qualify_release._prepare_output_directory(  # ruff: ignore[private-member-access]
                output,
            )
            try:
                self.assertEqual(staging.parent, output.parent)
                self.assertTrue(staging.is_dir())
                self.assertFalse(output.exists())
            finally:
                staging.rmdir()

    def test_prepare_creates_missing_parents_and_uses_destination_prefix(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "nested" / "deep" / "release-dist"
            staging = qualify_release._prepare_output_directory(  # ruff: ignore[private-member-access]
                output,
            )
            try:
                self.assertTrue(output.parent.is_dir())
                self.assertEqual(staging.parent, output.parent)
                self.assertTrue(staging.name.startswith(".release-dist."))
            finally:
                staging.rmdir()

    def test_lexical_absolute_resolves_only_the_parent_and_allows_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / "real"
            real.mkdir()
            alias = root / "alias"
            alias.symlink_to(real, target_is_directory=True)
            with patch.object(Path, "cwd", return_value=root):
                self.assertEqual(
                    qualify_release._lexical_absolute(  # ruff: ignore[private-member-access]
                        Path("alias") / "missing-output",
                    ),
                    real.resolve() / "missing-output",
                )

    def test_prepare_preserves_an_empty_destination_while_staging(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release-dist"
            output.mkdir()
            staging = qualify_release._prepare_output_directory(  # ruff: ignore[private-member-access]
                output,
            )
            try:
                self.assertTrue(output.is_dir())
                self.assertTrue(staging.is_dir())
            finally:
                staging.rmdir()

    def test_prepare_rejects_nonempty_non_directory_and_symbolic_destinations(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            nonempty = base / "nonempty"
            nonempty.mkdir()
            (nonempty / "public.txt").write_text("public", encoding="utf-8")
            regular = base / "regular"
            regular.write_text("public", encoding="utf-8")
            symbolic = base / "symbolic"
            symbolic.symlink_to(nonempty, target_is_directory=True)
            for output in (nonempty, regular, symbolic):
                with self.subTest(output=output.name):
                    self._assert_unsafe_output_rejected(output)

    def _assert_unsafe_output_rejected(self, output: Path) -> None:
        with self.assertRaisesRegex(
            qualify_release.ReleaseQualificationError,
            "absent or empty directory",
        ):
            qualify_release._prepare_output_directory(  # ruff: ignore[private-member-access]
                output,
            )

    def test_exact_entries_accept_only_expected_regular_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "alpha.bin").write_bytes(b"alpha")
            (base / "zeta.bin").write_bytes(b"zeta")
            qualify_release._assert_exact_entries(  # ruff: ignore[private-member-access]
                base,
                frozenset({"alpha.bin", "zeta.bin"}),
            )
            self.assertEqual(len(tuple(base.iterdir())), 2)

    def test_exact_entries_report_missing_and_unknown_names(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "unexpected.bin").write_bytes(b"public")
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                r"missing=\['expected.bin'\]; unknown=\['unexpected.bin'\]",
            ):
                qualify_release._assert_exact_entries(  # ruff: ignore[private-member-access]
                    base,
                    frozenset({"expected.bin"}),
                )

    def test_exact_entries_reject_symbolic_and_non_file_entries(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            target = base / "target.txt"
            target.write_text("public", encoding="utf-8")
            symbolic = base / "symbolic.bin"
            symbolic.symlink_to(target)
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "regular non-symbolic file",
            ):
                qualify_release._assert_exact_entries(  # ruff: ignore[private-member-access]
                    base,
                    frozenset({"symbolic.bin", "target.txt"}),
                )

        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "directory.bin").mkdir()
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "regular non-symbolic file",
            ):
                qualify_release._assert_exact_entries(  # ruff: ignore[private-member-access]
                    base,
                    frozenset({"directory.bin"}),
                )


class ReleaseChecksumTests(unittest.TestCase):
    """Exercise complete hashing and basename-only manifest verification."""

    def test_sha256_reads_a_complete_large_artifact(self) -> None:
        content = b"a" * 300_029
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "public.bin"
            artifact.write_bytes(content)
            digest = qualify_release._sha256(  # ruff: ignore[private-member-access]
                artifact,
            )
        self.assertEqual(digest, hashlib.sha256(content).hexdigest())

    def test_manifest_is_sorted_portable_and_verifies(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "zeta.bin").write_bytes(b"zeta")
            (base / "alpha.bin").write_bytes(b"alpha")
            manifest = qualify_release._write_and_verify_manifest(  # ruff: ignore[private-member-access]
                base,
                ("zeta.bin", "alpha.bin"),
            )
            lines = manifest.read_text(encoding="utf-8").splitlines()
        self.assertTrue(lines[0].endswith("  alpha.bin"))
        self.assertTrue(lines[1].endswith("  zeta.bin"))
        self.assertTrue(all("/" not in line and "\\" not in line for line in lines))

    def test_manifest_text_uses_exact_newline_separated_records(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "alpha.bin").write_bytes(b"alpha")
            (base / "zeta.bin").write_bytes(b"zeta")
            text = qualify_release._manifest_text(  # ruff: ignore[private-member-access]
                base,
                ("zeta.bin", "alpha.bin"),
            )
        self.assertEqual(
            text,
            f"{hashlib.sha256(b'alpha').hexdigest()}  alpha.bin\n"
            f"{hashlib.sha256(b'zeta').hexdigest()}  zeta.bin\n",
        )

    def test_manifest_io_uses_canonical_utf8_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            artifact_name = "public-ž.bin"
            (base / artifact_name).write_bytes(b"public")
            original_write = Path.write_bytes
            original_read = Path.read_bytes
            written_payloads: list[bytes] = []
            read_paths: list[Path] = []

            def recording_write(
                path: Path,
                data: bytes,
            ) -> int:
                written_payloads.append(data)
                return original_write(path, data)

            def recording_read(path: Path) -> bytes:
                read_paths.append(path)
                return original_read(path)

            with (
                patch.object(Path, "write_bytes", recording_write),
                patch.object(Path, "read_bytes", recording_read),
            ):
                manifest = qualify_release._write_and_verify_manifest(  # ruff: ignore[private-member-access]
                    base,
                    (artifact_name,),
                )
        self.assertEqual(len(written_payloads), 1)
        self.assertIn("public-ž.bin".encode(), written_payloads[0])
        self.assertEqual(read_paths, [manifest])

    def test_manifest_verification_rejects_changed_expected_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "public.bin").write_bytes(b"public")
            with (
                patch(
                    "tools.release_files.manifest_text",
                    side_effect=["first  public.bin\n", "second  public.bin\n"],
                ),
                self.assertRaisesRegex(
                    qualify_release.ReleaseQualificationError,
                    "failed verification",
                ),
            ):
                qualify_release._write_and_verify_manifest(  # ruff: ignore[private-member-access]
                    base,
                    ("public.bin",),
                )

    def test_existing_release_verifier_accepts_only_exact_matching_set(self) -> None:
        names = qualify_release._artifact_names(  # ruff: ignore[private-member-access]
            "public-project",
            "9.8.7",
        )
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / "release-dist"
            candidate.mkdir()
            for name in names:
                (candidate / name).write_bytes(f"public {name}".encode())
            qualify_release._write_and_verify_manifest(candidate, names)  # ruff: ignore[private-member-access]
            with (
                patch.object(
                    qualify_release,
                    "_project_identity",
                    return_value=("public-project", "9.8.7"),
                ),
                patch.object(
                    qualify_release,
                    "_verify_distribution_files",
                ) as verify_archives,
            ):
                verified = qualify_release.verify_release_directory(candidate)
                (candidate / names[0]).write_bytes(b"changed")
                with self.assertRaisesRegex(
                    qualify_release.ReleaseQualificationError,
                    "failed verification",
                ):
                    qualify_release.verify_release_directory(candidate)
            verify_archives.assert_called_once_with(
                candidate.resolve(),
                names,
                allow_staging=False,
            )
            self.assertEqual(
                {path.name for path in verified},
                {*names, qualify_release.CHECKSUM_FILE_NAME},
            )

    def test_existing_release_verifier_rejects_symbolic_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.mkdir()
            symbolic = Path(directory) / "symbolic"
            symbolic.symlink_to(target, target_is_directory=True)
            with self.assertRaisesRegex(
                qualify_release.ReleaseQualificationError,
                "non-symbolic directory",
            ):
                qualify_release.verify_release_directory(symbolic)


if __name__ == "__main__":
    unittest.main(verbosity=2)

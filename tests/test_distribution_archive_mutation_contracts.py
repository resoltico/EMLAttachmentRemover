"""Exact contracts for distribution-verifier mutation boundaries."""

from __future__ import annotations

import base64
import hashlib
import io
import tarfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import MagicMock, patch

from tools import verify_distribution_archives as verifier


class GeneratedMemberBoundaryTests(unittest.TestCase):
    """Lock the inclusive generated-member size limit."""

    def test_tar_member_at_exact_limit_is_read(self) -> None:
        archive = MagicMock()
        archive.extractfile.return_value = io.BytesIO(b"metadata")
        member = tarfile.TarInfo("public-1.0/PKG-INFO")
        member.size = verifier.MAX_GENERATED_FILE_BYTES

        actual = verifier._tar_file_bytes(  # ruff: ignore[private-member-access]
            archive,
            member,
        )

        self.assertEqual(actual, b"metadata")

    def test_zip_member_at_exact_limit_is_read(self) -> None:
        archive = MagicMock()
        archive.read.return_value = b"metadata"
        member = zipfile.ZipInfo("public-1.0.dist-info/METADATA")
        member.file_size = verifier.MAX_GENERATED_FILE_BYTES

        actual = verifier._zip_file_bytes(  # ruff: ignore[private-member-access]
            archive,
            member,
        )

        self.assertEqual(actual, b"metadata")


class ArchiveDelegationContractTests(unittest.TestCase):
    """Require safety-relevant arguments at archive helper boundaries."""

    def test_regular_tar_member_is_validated_as_a_file(self) -> None:
        member = tarfile.TarInfo("public-1.0/source.py")
        member.mtime = verifier.reproducibility.SOURCE_TIMESTAMP
        member.mode = verifier.reproducibility.SOURCE_MODE
        contract = MagicMock(source_root="public-1.0")
        files: dict[str, tarfile.TarInfo] = {}

        with patch.object(
            verifier.contract_module,
            "safe_parts",
            return_value=("public-1.0", "source.py"),
        ) as safe_parts:
            verifier._add_tar_member(  # ruff: ignore[private-member-access]
                member,
                contract,
                files,
                set(),
            )

        safe_parts.assert_called_once_with(member.name, directory=False)
        self.assertEqual(files, {"source.py": member})

    def test_tar_source_comparison_uses_bounded_chunks(self) -> None:
        archive = MagicMock()
        archived = io.BytesIO(b"source")
        archive.extractfile.return_value = archived
        source = Path("public-source.py")

        with patch.object(
            verifier.contract_module,
            "same_content",
            return_value=True,
        ) as same_content:
            verifier._compare_tar_sources(  # ruff: ignore[private-member-access]
                archive,
                {"source.py": tarfile.TarInfo("source.py")},
                {"source.py": source},
            )

        self.assertEqual(
            same_content.call_args.args,
            (archived, source, verifier.CHUNK_SIZE),
        )

    def test_zip_source_comparison_uses_bounded_chunks(self) -> None:
        archive = MagicMock()
        archived = MagicMock()
        opened = MagicMock()
        opened.__enter__.return_value = archived
        archive.open.return_value = opened
        source = Path("public-source.py")

        with patch.object(
            verifier.contract_module,
            "same_content",
            return_value=True,
        ) as same_content:
            verifier._compare_zip_sources(  # ruff: ignore[private-member-access]
                archive,
                {"source.py": zipfile.ZipInfo("source.py")},
                {"source.py": source},
            )

        self.assertEqual(
            same_content.call_args.args,
            (archived, source, verifier.CHUNK_SIZE),
        )

    def test_source_archive_is_opened_explicitly_as_gzip(self) -> None:
        archive_path = Path("public-1.0.tar.gz")
        opened = MagicMock()
        opened.__enter__.return_value = MagicMock()

        with (
            patch.object(tarfile, "open", return_value=opened) as open_archive,
            patch.object(
                verifier,
                "_inspect_source_archive",
                return_value=b"metadata",
            ),
        ):
            actual = verifier._verify_source_archive(  # ruff: ignore[private-member-access]
                archive_path,
                MagicMock(),
            )

        self.assertEqual(actual, b"metadata")
        open_archive.assert_called_once_with(archive_path, mode="r:gz")

    def test_source_inspection_uses_exact_diagnostic_labels(self) -> None:
        member = tarfile.TarInfo("public-1.0/PKG-INFO")
        archive = MagicMock()
        archive.getmembers.return_value = [member]
        contract = MagicMock(source_root="public-1.0", public_sources={})

        def classify(
            _member: tarfile.TarInfo,
            _contract: object,
            files: dict[str, tarfile.TarInfo],
            _directories: set[str],
        ) -> None:
            files["PKG-INFO"] = member

        with (
            patch.object(verifier, "_add_tar_member", side_effect=classify),
            patch.object(
                verifier.contract_module,
                "require_exact_set",
            ) as require_exact_set,
            patch.object(verifier, "_check_directories") as check_directories,
            patch.object(verifier, "_compare_tar_sources"),
            patch.object(verifier, "_tar_file_bytes", return_value=b"metadata"),
        ):
            actual = verifier._inspect_source_archive(  # ruff: ignore[private-member-access]
                archive,
                contract,
            )

        self.assertEqual(actual, b"metadata")
        self.assertEqual(
            require_exact_set.call_args.args,
            ("source archive", {"PKG-INFO"}, {"PKG-INFO"}),
        )
        self.assertEqual(
            check_directories.call_args.args,
            ("source-archive", set(), {"PKG-INFO"}),
        )


class WheelRecordContractTests(unittest.TestCase):
    """Exercise independent RECORD hash, size, and newline requirements."""

    def test_record_self_reference_rejects_each_populated_field(self) -> None:
        for values in (("sha256=unexpected", ""), ("", "1")):
            with self.subTest(values=values):
                with self.assertRaises(verifier.DistributionArchiveError) as caught:
                    verifier._verify_record_entry(  # ruff: ignore[private-member-access]
                        MagicMock(),
                        {"RECORD": zipfile.ZipInfo("RECORD")},
                        "RECORD",
                        "RECORD",
                        values,
                    )
                self.assertEqual(
                    str(caught.exception),
                    "wheel RECORD must not hash itself",
                )

    def test_record_rejects_hash_and_size_drift_independently(self) -> None:
        content = b"public"
        correct_hash = _record_hash(content)
        archive = MagicMock()
        archive.read.return_value = content
        member = zipfile.ZipInfo("public.py")
        member.file_size = len(content)
        cases = (("sha256=wrong", str(len(content))), (correct_hash, "999"))

        for values in cases:
            with (
                self.subTest(values=values),
                self.assertRaisesRegex(
                    verifier.DistributionArchiveError,
                    "digest or size mismatch",
                ),
            ):
                verifier._verify_record_entry(  # ruff: ignore[private-member-access]
                    archive,
                    {member.filename: member},
                    "RECORD",
                    member.filename,
                    values,
                )

    def test_record_accepts_cr_only_row_endings(self) -> None:
        content = b"public"
        record = (
            f"public.py,{_record_hash(content)},{len(content)}\rRECORD,,\r"
        ).encode()
        public_member = zipfile.ZipInfo("public.py")
        public_member.file_size = len(content)
        record_member = zipfile.ZipInfo("RECORD")
        record_member.file_size = len(record)
        archive = MagicMock()
        archive.read.side_effect = lambda member: (
            record if member is record_member else content
        )

        verifier._verify_record(  # ruff: ignore[private-member-access]
            archive,
            {"public.py": public_member, "RECORD": record_member},
            "RECORD",
        )
        self.assertEqual(archive.read.call_count, 2)

    def test_record_parser_preserves_embedded_carriage_returns(self) -> None:
        record = b'"bad\rcandidate",,\n"bad\ncandidate",,\nRECORD,,\n'
        archive = MagicMock()
        archive.read.return_value = record
        member = zipfile.ZipInfo("RECORD")
        member.file_size = len(record)

        with self.assertRaises(verifier.DistributionArchiveError) as caught:
            verifier._verify_record(  # ruff: ignore[private-member-access]
                archive,
                {"RECORD": member},
                "RECORD",
            )

        self.assertEqual(
            str(caught.exception),
            "wheel RECORD member mismatch: missing=[]; "
            "unknown=['bad\\ncandidate', 'bad\\rcandidate']",
        )

    def test_record_structure_errors_have_exact_public_diagnostics(self) -> None:
        cases = (
            (b"only,two\n", "wheel RECORD rows must have exactly 3 fields"),
            (b"same,,\nsame,,\n", "wheel RECORD contains duplicate paths"),
        )
        for record, expected in cases:
            with self.subTest(expected=expected):
                archive = MagicMock()
                archive.read.return_value = record
                member = zipfile.ZipInfo("RECORD")
                member.file_size = len(record)
                with self.assertRaises(verifier.DistributionArchiveError) as caught:
                    verifier._verify_record(  # ruff: ignore[private-member-access]
                        archive,
                        {"RECORD": member},
                        "RECORD",
                    )
                self.assertEqual(str(caught.exception), expected)

    def test_record_set_check_uses_exact_diagnostic_label(self) -> None:
        record = b"RECORD,,\n"
        archive = MagicMock()
        archive.read.return_value = record
        member = zipfile.ZipInfo("RECORD")
        member.file_size = len(record)

        with (
            patch.object(
                verifier.contract_module,
                "require_exact_set",
            ) as require_exact_set,
            patch.object(verifier, "_verify_record_entry"),
        ):
            verifier._verify_record(  # ruff: ignore[private-member-access]
                archive,
                {"RECORD": member},
                "RECORD",
            )

        self.assertEqual(
            require_exact_set.call_args.args,
            ("wheel RECORD", {"RECORD"}, {"RECORD"}),
        )


class WheelRootContractTests(unittest.TestCase):
    """Require wheel roots to mean the first path component."""

    def test_nested_wheel_source_keeps_only_its_top_level_root(self) -> None:
        dist_info = "public-1.0.dist-info"
        names = {
            "public/nested/module.py",
            f"{dist_info}/METADATA",
            f"{dist_info}/RECORD",
            f"{dist_info}/WHEEL",
        }
        members = [zipfile.ZipInfo(name) for name in names]
        archive = MagicMock()
        archive.infolist.return_value = members
        contract = MagicMock(dist_info=dist_info)
        contract.wheel_sources.return_value = {
            "public/nested/module.py": Path("module.py"),
        }
        contract.wheel_generated_files.return_value = frozenset(
            names
            - {
                "public/nested/module.py",
            }
        )
        observed_roots: list[set[str]] = []

        def classify(
            member: zipfile.ZipInfo,
            roots: set[str],
            files: dict[str, zipfile.ZipInfo],
            _directories: set[str],
        ) -> None:
            observed_roots.append(roots.copy())
            files[member.filename] = member

        with (
            patch.object(verifier, "_add_zip_member", side_effect=classify),
            patch.object(
                verifier.contract_module,
                "require_exact_set",
            ) as require_exact_set,
            patch.object(verifier, "_check_directories") as check_directories,
            patch.object(verifier, "_compare_zip_sources"),
            patch.object(verifier, "_zip_file_bytes", return_value=b"metadata"),
            patch.object(verifier, "_verify_record"),
            patch.object(verifier, "_verify_generated_wheel_files"),
        ):
            actual = verifier._inspect_wheel(  # ruff: ignore[private-member-access]
                archive,
                contract,
            )

        self.assertEqual(actual, b"metadata")
        self.assertEqual(
            observed_roots,
            [{"public", dist_info}] * len(members),
        )
        self.assertEqual(require_exact_set.call_args.args[0], "wheel")
        self.assertEqual(check_directories.call_args.args[0], "wheel")


def _record_hash(content: bytes) -> str:
    """Return the canonical unpadded wheel RECORD SHA-256 value.

    Returns:
        The URL-safe SHA-256 RECORD field.

    """
    encoded = base64.urlsafe_b64encode(hashlib.sha256(content).digest())
    return f"sha256={encoded.rstrip(b'=').decode()}"


if __name__ == "__main__":
    unittest.main(verbosity=2)

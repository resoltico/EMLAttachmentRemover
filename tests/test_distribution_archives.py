"""Adversarial behavioral tests for source and wheel archive verification."""

from __future__ import annotations

import tarfile
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path
from typing import Protocol
from unittest.mock import MagicMock, patch

from tools import verify_distribution_archives as verifier

from tests.distribution_archive_support import (
    create_distribution,
    write_source_archive,
    write_wheel,
)


class DistributionFixture(Protocol):
    """Describe generated paths consumed by the public verifier."""

    source_archive: Path
    wheel: Path
    root: Path
    config: Path
    public_files: tuple[Path, ...]


def _verify(synthetic: DistributionFixture) -> None:
    verifier.verify_distribution_archives(
        synthetic.source_archive,
        synthetic.wheel,
        synthetic.root,
        synthetic.config,
        synthetic.public_files,
    )


def _append_wheel_member(wheel: Path, name: str, mode: int) -> None:
    """Append one synthetic adversarial wheel member."""
    with zipfile.ZipFile(wheel, "a") as archive, warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        info = zipfile.ZipInfo(name)
        info.external_attr = mode << 16
        archive.writestr(info, b"public")


class DistributionArchiveIntegrationTests(unittest.TestCase):
    """Exercise valid archives and cross-archive metadata consistency."""

    def test_valid_synthetic_distribution_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _verify(distribution)
            self.assertTrue(distribution.source_archive.is_file())

    def test_mismatched_sdist_and_wheel_metadata_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            write_wheel(
                distribution.wheel,
                distribution.contract,
                distribution.metadata.replace(b"Version: 1.2.3", b"Version: 9.9.9"),
            )
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError,
                "^sdist PKG-INFO and wheel METADATA differ$",
            ):
                _verify(distribution)


class SourceArchiveTests(unittest.TestCase):
    """Exercise unsafe, inexact, and corrupted sdist surfaces."""

    def test_source_rejects_unsafe_wrong_root_duplicate_and_link_members(self) -> None:
        cases: tuple[tuple[str, tarfile.TarInfo], ...] = (
            ("unsafe", tarfile.TarInfo("../outside")),
            ("wrong source-archive root", tarfile.TarInfo("other/extra")),
            (
                "duplicate source-archive member",
                tarfile.TarInfo("public_project-1.2.3/LICENSE"),
            ),
            ("non-regular source-archive member", _link_member()),
        )
        for expected, member in cases:
            with (
                self.subTest(expected=expected),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                write_source_archive(
                    distribution.source_archive,
                    distribution.contract,
                    distribution.metadata,
                    extra_members=(member,),
                )
                with self.assertRaisesRegex(
                    verifier.DistributionArchiveError, expected
                ):
                    _verify(distribution)

    def test_source_rejects_unknown_directory_and_content_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            unknown = tarfile.TarInfo(f"{distribution.contract.source_root}/unused")
            unknown.type = tarfile.DIRTYPE
            unknown.mtime = verifier.reproducibility.SOURCE_TIMESTAMP
            unknown.mode = verifier.reproducibility.SOURCE_MODE
            write_source_archive(
                distribution.source_archive,
                distribution.contract,
                distribution.metadata,
                extra_members=(unknown,),
            )
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError, "unexpected"
            ):
                _verify(distribution)
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            (distribution.root / "LICENSE").write_text("changed\n", encoding="utf-8")
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError,
                "source-archive content differs",
            ):
                _verify(distribution)

    def test_source_rejects_corrupt_and_oversized_generated_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            distribution.source_archive.write_bytes(b"not a source archive")
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError, "cannot inspect"
            ):
                _verify(distribution)
        member = MagicMock(size=verifier.MAX_GENERATED_FILE_BYTES + 1, name="PKG-INFO")
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "too large"):
            verifier._tar_file_bytes(MagicMock(), member)  # ruff: ignore[private-member-access]

    def test_source_rejects_non_reproducible_member_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            extra = tarfile.TarInfo(f"{distribution.contract.source_root}/extra")
            extra.size = 0
            write_source_archive(
                distribution.source_archive,
                distribution.contract,
                distribution.metadata,
                extra_members=(extra,),
            )
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError,
                "non-reproducible source-archive metadata",
            ):
                _verify(distribution)

    def test_source_rejects_unreadable_regular_member(self) -> None:
        member = MagicMock(size=1, name="PKG-INFO")
        archive = MagicMock()
        archive.extractfile.return_value = None
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "cannot read"):
            verifier._tar_file_bytes(archive, member)  # ruff: ignore[private-member-access]

    def test_source_rejects_root_file_and_unreadable_repository_member(self) -> None:
        root_file = tarfile.TarInfo("public_project-1.2.3")
        contract = MagicMock(source_root="public_project-1.2.3")
        with self.assertRaisesRegex(
            verifier.DistributionArchiveError, "must be a directory"
        ):
            verifier._add_tar_member(  # ruff: ignore[private-member-access]
                root_file,
                contract,
                {},
                set(),
            )
        archive = MagicMock()
        archive.extractfile.return_value = None
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "cannot read"):
            verifier._compare_tar_sources(  # ruff: ignore[private-member-access]
                archive,
                {"public": MagicMock()},
                {"public": Path("public")},
            )


def _link_member() -> tarfile.TarInfo:
    """Return one prohibited public synthetic tar link.

    Returns:
        A link member under the expected archive root.

    """
    member = tarfile.TarInfo("public_project-1.2.3/link")
    member.type = tarfile.SYMTYPE
    member.linkname = "LICENSE"
    return member


class WheelArchiveTests(unittest.TestCase):
    """Exercise unsafe, inexact, and semantically drifting wheels."""

    def test_wheel_rejects_unsafe_duplicate_wrong_root_and_links(self) -> None:
        cases = (
            ("unsafe", "../outside", 0),
            ("wrong wheel member root", "other/file", 0),
            (
                "duplicate wheel member",
                "public_package/__init__.py",
                0,
            ),
            ("non-regular wheel member", "public_package/link", 0o120777),
        )
        for expected, name, mode in cases:
            with (
                self.subTest(expected=expected),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _append_wheel_member(distribution.wheel, name, mode)
                with self.assertRaisesRegex(
                    verifier.DistributionArchiveError, expected
                ):
                    _verify(distribution)

    def test_wheel_rejects_encrypted_member_before_reading(self) -> None:
        member = MagicMock(flag_bits=1, filename="public/file", is_dir=lambda: False)
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "encrypted"):
            verifier._add_zip_member(  # ruff: ignore[private-member-access]
                member,
                {"public"},
                {},
                set(),
            )

    def test_wheel_rejects_non_reproducible_member_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _append_wheel_member(distribution.wheel, "public_package/extra", 0o100644)
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError,
                "non-reproducible wheel member metadata",
            ):
                _verify(distribution)

    def test_wheel_accepts_explicit_safe_directory_and_rejects_encrypted_read(
        self,
    ) -> None:
        member = MagicMock(
            flag_bits=0,
            filename="public/",
            date_time=verifier.reproducibility.ZIP_TIMESTAMP,
            create_system=verifier.reproducibility.UNIX_ZIP_SYSTEM,
            external_attr=0o040755 << 16,
            is_dir=lambda: True,
        )
        directories: set[str] = set()
        verifier._add_zip_member(  # ruff: ignore[private-member-access]
            member,
            {"public"},
            {},
            directories,
        )
        self.assertEqual(directories, {"public"})
        encrypted = MagicMock(flag_bits=1, filename="public", file_size=1)
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "encrypted"):
            verifier._zip_file_bytes(  # ruff: ignore[private-member-access]
                MagicMock(),
                encrypted,
            )

    def test_wheel_rejects_source_entrypoint_and_compatibility_drift(self) -> None:
        cases = (
            ("wheel content differs", {"public_package/__init__.py": b"changed"}),
            (
                "console entry points mismatch",
                {
                    "public_project-1.2.3.dist-info/entry_points.txt": (
                        b"[console_scripts]\nchanged = public:main\n"
                    ),
                },
            ),
            (
                "compatibility contract mismatch",
                {
                    "public_project-1.2.3.dist-info/WHEEL": (
                        b"Wheel-Version: 1.0\nTag: cp314-cp314-any\n"
                    ),
                },
            ),
        )
        for expected, overrides in cases:
            with (
                self.subTest(expected=expected),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                write_wheel(
                    distribution.wheel,
                    distribution.contract,
                    distribution.metadata,
                    overrides=overrides,
                )
                with self.assertRaisesRegex(
                    verifier.DistributionArchiveError, expected
                ):
                    _verify(distribution)

    def test_wheel_rejects_bad_record_rows_duplicates_and_hashes(self) -> None:
        cases = (
            ("exactly 3 fields", b"only,two\n"),
            ("duplicate paths", b"a,,\na,,\n"),
            ("member mismatch", b"a,,\n"),
        )
        for expected, record in cases:
            with self.subTest(expected=expected):
                archive = MagicMock()
                archive.read.return_value = record
                member = MagicMock(
                    flag_bits=0, file_size=len(record), filename="RECORD"
                )
                with self.assertRaisesRegex(
                    verifier.DistributionArchiveError, expected
                ):
                    verifier._verify_record(  # ruff: ignore[private-member-access]
                        archive,
                        {"RECORD": member},
                        "RECORD",
                    )

    def test_wheel_wraps_invalid_record_encoding(self) -> None:
        archive = MagicMock()
        archive.read.return_value = b"\xff"
        member = MagicMock(flag_bits=0, file_size=1, filename="RECORD")
        with self.assertRaisesRegex(
            verifier.DistributionArchiveError, "valid UTF-8 CSV"
        ):
            verifier._verify_record(  # ruff: ignore[private-member-access]
                archive,
                {"RECORD": member},
                "RECORD",
            )

    def test_wheel_rejects_hashed_record_itself_and_wrong_member_digest(self) -> None:
        archive = MagicMock()
        archive.read.return_value = b"public"
        member = MagicMock(flag_bits=0, file_size=1, filename="RECORD")
        for values, expected in (
            (("sha256=public", "1"), "must not hash itself"),
            (("sha256=wrong", "1"), "digest or size mismatch"),
        ):
            name = "RECORD" if "itself" in expected else "public"
            members = {name: member}
            with (
                self.subTest(expected=expected),
                self.assertRaisesRegex(
                    verifier.DistributionArchiveError,
                    expected,
                ),
            ):
                verifier._verify_record_entry(  # ruff: ignore[private-member-access]
                    archive,
                    members,
                    "RECORD",
                    name,
                    values,
                )

    def test_wheel_rejects_oversized_generated_member_and_bad_zip(self) -> None:
        member = MagicMock(
            flag_bits=0,
            file_size=verifier.MAX_GENERATED_FILE_BYTES + 1,
            filename="METADATA",
        )
        with self.assertRaisesRegex(verifier.DistributionArchiveError, "too large"):
            verifier._zip_file_bytes(MagicMock(), member)  # ruff: ignore[private-member-access]
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            distribution.wheel.write_bytes(b"not a wheel")
            with self.assertRaisesRegex(
                verifier.DistributionArchiveError, "cannot inspect"
            ):
                _verify(distribution)

    def test_wheel_without_entrypoints_and_metadata_parse_error_branches(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory), scripts=False)
            _verify(distribution)
            self.assertFalse(distribution.contract.has_scripts)
        parser = MagicMock()
        parser.parsebytes.side_effect = ValueError("invalid metadata")
        with (
            patch.object(verifier, "BytesParser", return_value=parser),
            self.assertRaisesRegex(verifier.DistributionArchiveError, "cannot parse"),
        ):
            verifier._parse_metadata(b"public")  # ruff: ignore[private-member-access]


if __name__ == "__main__":
    unittest.main(verbosity=2)

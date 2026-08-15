"""Behavioral tests for the deterministic zipapp build tool."""

from __future__ import annotations

import hashlib
import runpy
import sys
import tempfile
import unittest
import zipfile
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

from tools import build_zipapp, mutmut_workspace

PUBLIC_METADATA = build_zipapp.ProjectMetadata(
    name="public-project",
    version="9.8.7",
    summary="Public summary",
    requires_python=">=3.14,<3.15",
    license_expression="MIT",
    implementation="CPython",
)


class ZipappSourceTests(unittest.TestCase):
    """Exercise project metadata and archive-member discovery."""

    def test_direct_script_entrypoint_exposes_help(self) -> None:
        """Keep the build tool's portable direct-execution import path usable."""
        path = Path(build_zipapp.__file__).resolve()
        stdout = StringIO()
        with (
            patch.object(sys, "argv", [str(path), "--help"]),
            patch.object(sys, "path", [str(path.parent), *sys.path]),
            redirect_stdout(stdout),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")

        self.assertEqual(raised.exception.code, 0)
        self.assertIn("usage:", stdout.getvalue())

    def test_archive_metadata_comes_entirely_from_project_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "pyproject.toml"
            config.write_text(
                "[project]\n"
                'name = "public-project"\n'
                'version = "9.8.7"\n'
                'description = "Public summary"\n'
                'requires-python = ">=3.14,<3.15"\n'
                'license = "MIT"\n'
                "[tool.eml-attachment-remover.runtime]\n"
                'implementation = "CPython"\n',
                encoding="utf-8",
            )
            with patch.object(build_zipapp, "PROJECT_CONFIG", config):
                metadata = build_zipapp._project_metadata()  # ruff: ignore[private-member-access]
        self.assertEqual(metadata, PUBLIC_METADATA)

    def test_runtime_bounds_and_launcher_enforce_both_boundaries(self) -> None:
        lower, upper = build_zipapp._runtime_bounds(  # ruff: ignore[private-member-access]
            PUBLIC_METADATA.requires_python
        )
        self.assertFalse(lower <= (3, 13) < upper)
        self.assertTrue(lower <= (3, 14) < upper)
        self.assertFalse(lower <= (3, 15) < upper)
        launcher = build_zipapp._launcher_source(PUBLIC_METADATA).decode()  # ruff: ignore[private-member-access]
        self.assertIn("python_implementation() != 'CPython'", launcher)
        self.assertIn("(3, 14) <= sys.version_info[:2] < (3, 15)", launcher)
        with self.assertRaisesRegex(ValueError, "unsupported requires-python"):
            build_zipapp._runtime_bounds(">=3.14")  # ruff: ignore[private-member-access]

    def test_source_files_are_filtered_and_portably_sorted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            (package / "nested").mkdir(parents=True)
            (package / "__pycache__").mkdir()
            (package / "__pycache__" / "cached.py").write_text(
                "CACHED = True\n",
                encoding="utf-8",
            )
            (package / "zeta.py").write_text("ZETA = 1\n", encoding="utf-8")
            (package / "nested" / "alpha.py").write_text(
                "ALPHA = 1\n",
                encoding="utf-8",
            )
            (package / "py.typed").write_bytes(b"")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
            ):
                sources = build_zipapp._source_files()  # ruff: ignore[private-member-access]
        self.assertEqual(
            [source.relative_to(package).as_posix() for source in sources],
            ["nested/alpha.py", "py.typed", "zeta.py"],
        )

    def test_source_file_order_uses_portable_relative_names(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            (package / "b").mkdir(parents=True)
            (package / "a.py").write_text("A = 1\n", encoding="utf-8")
            (package / "b" / "z.py").write_text("Z = 1\n", encoding="utf-8")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                patch.object(Path, "__lt__", side_effect=AssertionError("path order")),
            ):
                sources = build_zipapp._source_files()  # ruff: ignore[private-member-access]
        self.assertEqual(
            [source.relative_to(package).as_posix() for source in sources],
            ["a.py", "b/z.py"],
        )

    def test_source_files_reject_unexpected_and_symbolic_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            package.mkdir(parents=True)
            unexpected = package / "private.bin"
            unexpected.write_bytes(b"public synthetic bytes")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                self.assertRaisesRegex(ValueError, "unexpected package source file"),
            ):
                build_zipapp._source_files()  # ruff: ignore[private-member-access]
            unexpected.unlink()
            external = root / "external.py"
            external.write_text("PUBLIC = 1\n", encoding="utf-8")
            (package / "symbolic.py").symlink_to(external)
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                self.assertRaisesRegex(ValueError, "symbolic link"),
            ):
                build_zipapp._source_files()  # ruff: ignore[private-member-access]

    def test_source_files_reject_non_regular_entries_defensively(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            package.mkdir(parents=True)
            (package / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                patch.object(Path, "is_file", return_value=False),
                self.assertRaisesRegex(ValueError, "regular file"),
            ):
                build_zipapp._source_files()  # ruff: ignore[private-member-access]

    def test_source_files_reject_a_missing_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"
            with patch.object(build_zipapp, "PACKAGE_SOURCE", missing):
                with self.assertRaisesRegex(FileNotFoundError, "package source"):
                    build_zipapp._source_files()  # ruff: ignore[private-member-access]

    def test_archive_members_include_license_launcher_metadata_and_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            package.mkdir(parents=True)
            (package / "__init__.py").write_bytes(b'VALUE = "public"\n')
            license_file = root / "LICENSE"
            license_file.write_bytes(b"Public license\n")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                patch.object(build_zipapp, "LICENSE_FILE", license_file),
            ):
                members = build_zipapp._archive_members(PUBLIC_METADATA)  # ruff: ignore[private-member-access]
        names = [name for name, _content in members]
        self.assertEqual(names, sorted(names))
        self.assertEqual(
            dict(members)["eml_attachment_remover/__init__.py"],
            b'VALUE = "public"\n',
        )
        self.assertEqual(dict(members)["LICENSE"], b"Public license\n")
        metadata_name = "public_project-9.8.7.dist-info/METADATA"
        self.assertEqual(
            dict(members)[metadata_name],
            b"Metadata-Version: 2.4\n"
            b"Name: public-project\n"
            b"Version: 9.8.7\n"
            b"Summary: Public summary\n"
            b"Requires-Python: >=3.14,<3.15\n"
            b"License-Expression: MIT\n",
        )
        self.assertIn(b"raise SystemExit", dict(members)["__main__.py"])

    def test_archive_member_sorting_uses_names_before_byte_payloads(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "src" / "eml_attachment_remover"
            package.mkdir(parents=True)
            source = package / "zeta.py"
            source.write_bytes(b"A")
            license_file = root / "LICENSE"
            license_file.write_bytes(b"Z")
            with (
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                patch.object(build_zipapp, "LICENSE_FILE", license_file),
            ):
                members = build_zipapp._archive_members(PUBLIC_METADATA)  # ruff: ignore[private-member-access]
        self.assertEqual([name for name, _content in members], sorted(dict(members)))

    def test_archive_uses_mutant_local_source_and_omits_exact_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            (parent / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
            root = parent / "mutants"
            package = root / "src" / "eml_attachment_remover"
            package.mkdir(parents=True)
            source = package / "__init__.py"
            source.write_bytes(b'MUTANT_LOCAL = "instrumented"\n')
            (package / "__init__.py.meta").write_text("{}\n", encoding="utf-8")
            (package / "__init__.py.spans").write_text("{}\n", encoding="utf-8")
            license_file = root / "LICENSE"
            license_file.write_bytes(b"Public license\n")
            original_sidecar = mutmut_workspace.generated_sidecar

            def generated_sidecar(path: Path, project_root: Path) -> bool:
                return original_sidecar(path, project_root, marker="")

            with (
                patch.object(build_zipapp, "generated_sidecar", generated_sidecar),
                patch.object(build_zipapp, "PROJECT_ROOT", root),
                patch.object(build_zipapp, "PACKAGE_SOURCE", package),
                patch.object(build_zipapp, "LICENSE_FILE", license_file),
            ):
                members = dict(build_zipapp._archive_members(PUBLIC_METADATA))  # ruff: ignore[private-member-access]

        self.assertEqual(
            members["eml_attachment_remover/__init__.py"],
            b'MUTANT_LOCAL = "instrumented"\n',
        )
        self.assertFalse(any(name.endswith((".meta", ".spans")) for name in members))

    def test_archive_members_reject_a_missing_license(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "LICENSE"
            with patch.object(build_zipapp, "LICENSE_FILE", missing):
                with self.assertRaisesRegex(FileNotFoundError, "license file"):
                    build_zipapp._archive_members(PUBLIC_METADATA)  # ruff: ignore[private-member-access]


class ZipappArchiveTests(unittest.TestCase):
    """Exercise deterministic archive writing, verification, and hashing."""

    def test_zip_info_normalizes_every_reproducibility_field(self) -> None:
        info = build_zipapp._zip_info("public.txt")  # ruff: ignore[private-member-access]
        self.assertEqual(info.filename, "public.txt")
        self.assertEqual(info.date_time, (1980, 1, 1, 0, 0, 0))
        self.assertEqual(info.compress_type, zipfile.ZIP_STORED)
        self.assertEqual(info.create_system, 3)
        self.assertEqual(info.external_attr, build_zipapp.ARCHIVE_MODE)

    def test_temporary_path_is_an_empty_sibling(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "public.pyz"
            temporary = build_zipapp._temporary_path(target)  # ruff: ignore[private-member-access]
            try:
                self.assertEqual(temporary.parent, target.parent)
                self.assertTrue(temporary.is_file())
                self.assertEqual(temporary.stat().st_size, 0)
                self.assertTrue(temporary.name.startswith(".public.pyz."))
                self.assertTrue(temporary.name.endswith(".tmp"))
            finally:
                temporary.unlink(missing_ok=True)

    def test_write_archive_prepends_launcher_and_writes_members(self) -> None:
        members = (("alpha.txt", b"alpha"), ("zeta.txt", b"zeta"))
        with tempfile.TemporaryDirectory() as directory:
            archive_path = Path(directory) / "public.pyz"
            with patch.object(build_zipapp, "_archive_members", return_value=members):
                build_zipapp._write_archive(archive_path, PUBLIC_METADATA)  # ruff: ignore[private-member-access]
            self.assertTrue(
                archive_path.read_bytes().startswith(
                    f"#!{build_zipapp.INTERPRETER}\n".encode(),
                ),
            )
            with zipfile.ZipFile(archive_path) as archive:
                self.assertEqual(archive.namelist(), ["alpha.txt", "zeta.txt"])
                self.assertEqual(archive.read("alpha.txt"), b"alpha")
                self.assertTrue(
                    all(
                        member.compress_type == zipfile.ZIP_STORED
                        for member in archive.infolist()
                    )
                )

    def test_verify_archive_checks_integrity_metadata_and_execution(self) -> None:
        members = (("public.txt", b"public"),)
        with tempfile.TemporaryDirectory() as directory:
            archive_path = Path(directory) / "public.pyz"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr(*members[0])
            with patch.object(build_zipapp, "_archive_members", return_value=members):
                build_zipapp._verify_archive(  # ruff: ignore[private-member-access]
                    archive_path,
                    PUBLIC_METADATA,
                    execute=False,
                )
                with patch("tools.build_zipapp.subprocess.run") as run:
                    build_zipapp._verify_archive(  # ruff: ignore[private-member-access]
                        archive_path,
                        PUBLIC_METADATA,
                        execute=True,
                    )
        run.assert_called_once_with(
            [
                sys.executable,
                "-I",
                "-X",
                "dev",
                "-W",
                "error",
                str(archive_path),
                "--version",
            ],
            check=True,
            text=True,
            timeout=build_zipapp.VERIFY_TIMEOUT_SECONDS,
        )
        self.assertEqual(run.call_count, 1)

    def test_verify_archive_rejects_missing_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive_path = Path(directory) / "public.pyz"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr("public.txt", b"public")
            with (
                patch.object(build_zipapp, "_archive_members", return_value=()),
                self.assertRaisesRegex(RuntimeError, "invalid zipapp archive"),
            ):
                build_zipapp._verify_archive(  # ruff: ignore[private-member-access]
                    archive_path,
                    PUBLIC_METADATA,
                    execute=False,
                )

    def test_verify_archive_rejects_a_corrupt_member(self) -> None:
        members = (("public.txt", b"public"),)
        with tempfile.TemporaryDirectory() as directory:
            archive_path = Path(directory) / "public.pyz"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr(*members[0])
            with (
                patch.object(build_zipapp, "_archive_members", return_value=members),
                patch.object(zipfile.ZipFile, "testzip", return_value="public.txt"),
                self.assertRaisesRegex(RuntimeError, "invalid zipapp archive"),
            ):
                build_zipapp._verify_archive(  # ruff: ignore[private-member-access]
                    archive_path,
                    PUBLIC_METADATA,
                    execute=False,
                )

    def test_verify_archive_rejects_changed_and_extra_members(self) -> None:
        expected = (("public.txt", b"public"),)
        for members in (
            (("public.txt", b"changed"),),
            (("public.txt", b"public"), ("extra.txt", b"extra")),
        ):
            with tempfile.TemporaryDirectory() as directory:
                self._assert_invalid_members(Path(directory), members, expected)

    def _assert_invalid_members(
        self,
        directory: Path,
        members: tuple[tuple[str, bytes], ...],
        expected: tuple[tuple[str, bytes], ...],
    ) -> None:
        archive_path = directory / "public.pyz"
        with zipfile.ZipFile(archive_path, "w") as archive:
            for name, content in members:
                archive.writestr(name, content)
        with (
            patch.object(build_zipapp, "_archive_members", return_value=expected),
            self.assertRaisesRegex(RuntimeError, "invalid zipapp archive"),
        ):
            build_zipapp._verify_archive(  # ruff: ignore[private-member-access]
                archive_path,
                PUBLIC_METADATA,
                execute=False,
            )

    def test_sha256_reads_the_complete_file_in_bounded_chunks(self) -> None:
        content = b"a" * (build_zipapp.CHUNK_SIZE + 17)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "public.bin"
            path.write_bytes(content)
            digest = build_zipapp._sha256(path)  # ruff: ignore[private-member-access]
        self.assertEqual(digest, hashlib.sha256(content).hexdigest())

    def test_sha256_uses_the_declared_bounded_chunk_size(self) -> None:
        path = Path("/public/archive.pyz")
        archive = unittest.mock.MagicMock()
        archive.__enter__.return_value = archive
        archive.read.side_effect = [b"public", b""]
        with patch.object(Path, "open", return_value=archive):
            build_zipapp._sha256(path)  # ruff: ignore[private-member-access]
        self.assertEqual(
            archive.read.call_args_list,
            [
                unittest.mock.call(build_zipapp.CHUNK_SIZE),
                unittest.mock.call(build_zipapp.CHUNK_SIZE),
            ],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)

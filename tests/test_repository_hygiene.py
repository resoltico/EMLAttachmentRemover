"""Regression tests for the public-repository hygiene auditor."""

from __future__ import annotations

import io
import os
import stat
import tempfile
import tomllib
import unittest
from os import stat_result
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import check_repository_hygiene as hygiene

from tests.repository_hygiene_support import issue_messages as _messages
from tests.repository_hygiene_support import make_public_root as _root

if TYPE_CHECKING:
    from collections.abc import Iterator


class RepositoryHygieneTests(unittest.TestCase):
    """Exercise the manifest and every public-workspace policy category."""

    def test_clean_manifest_is_sorted_and_excludes_only_generated_paths(self) -> None:
        configuration = tomllib.loads(
            (hygiene.PROJECT_ROOT / "pyproject.toml").read_text(encoding="utf-8")
        )
        sdist_includes = set(
            configuration["tool"]["hatch"]["build"]["targets"]["sdist"]["include"]
        )
        expected_includes = {
            f"/{name}"
            for name in hygiene.PUBLIC_ROOT_FILES | hygiene.PUBLIC_ROOT_DIRECTORIES
        }
        self.assertEqual(sdist_includes, expected_includes)
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            (root / "src" / "public.py").write_text("PUBLIC = True\n")
            (root / "src" / "package.egg-info").mkdir()
            (root / "mutants").mkdir()
            audit = hygiene.audit_repository(root)
        names = [path.relative_to(root).as_posix() for path in audit.public_files]
        self.assertEqual(audit.issues, ())
        self.assertEqual(names, sorted(names))
        self.assertIn("src/public.py", names)
        self.assertFalse(any("cache" in name or "egg-info" in name for name in names))

    def test_generated_file_categories_are_excluded_from_public_directories(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            for name in (".DS_Store", "public.pyc", "public.pyo"):
                (root / "src" / name).write_bytes(b"generated")

            audit = hygiene.audit_repository(root)

        names = {path.name for path in audit.public_files}
        self.assertTrue(names.isdisjoint({".DS_Store", "public.pyc", "public.pyo"}))
        bytecode_issues = {
            issue.path.name: issue.message
            for issue in audit.issues
            if issue.path.suffix in {".pyc", ".pyo"}
        }
        self.assertEqual(
            bytecode_issues,
            {
                "public.pyc": (
                    "Python bytecode cache is prohibited because it can embed "
                    "machine-specific paths; run Python with bytecode disabled and "
                    "remove this path"
                ),
                "public.pyo": (
                    "Python bytecode cache is prohibited because it can embed "
                    "machine-specific paths; run Python with bytecode disabled and "
                    "remove this path"
                ),
            },
        )

    def test_public_directory_listing_error_is_an_issue(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            original_iterdir = Path.iterdir

            def fail_for_source(path: Path) -> Iterator[Path]:
                if path == root / "src":
                    message = "denied"
                    raise OSError(message)
                return original_iterdir(path)

            with patch.object(Path, "iterdir", fail_for_source):
                audit = hygiene.audit_repository(root)

        self.assertTrue(
            any("cannot list directory" in item for item in _messages(audit))
        )

    def test_nested_public_directory_is_traversed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            nested = root / "src" / "public-package"
            nested.mkdir()
            public = nested / "public.py"
            public.write_text("PUBLIC = True\n", encoding="utf-8")

            audit = hygiene.audit_repository(root)

        self.assertIn(public, audit.public_files)
        self.assertFalse(audit.issues)

    def test_failed_child_inspection_continues_to_next_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            blocked = root / "src" / "blocked.py"
            retained = root / "src" / "retained.py"
            blocked.write_text("PUBLIC", encoding="utf-8")
            retained.write_text("PUBLIC", encoding="utf-8")
            original_lstat = Path.lstat

            def fail_one(path: Path) -> stat_result:
                if path == blocked:
                    message = "denied"
                    raise OSError(message)
                return original_lstat(path)

            with patch.object(Path, "lstat", fail_one):
                audit = hygiene.audit_repository(root)

        self.assertIn(retained, audit.public_files)
        self.assertTrue(any("cannot inspect" in item for item in _messages(audit)))

    def test_ignored_private_and_unexpected_paths_are_all_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            for name in (".env", "mail.eml", "id_ed25519", "credentials.json"):
                (root / name).write_text("PRIVATE", encoding="utf-8")
            (root / "src" / "archive.pst").write_text("PRIVATE", encoding="utf-8")
            (root / "src" / "certificate.pem").write_text("PRIVATE", encoding="utf-8")
            (root / "undocumented").mkdir()
            audit = hygiene.audit_repository(root)
        messages = _messages(audit)
        self.assertTrue(any("environment" in message for message in messages))
        self.assertTrue(any("mail format" in message for message in messages))
        self.assertTrue(any("private-key" in message for message in messages))
        self.assertTrue(any("credential-bearing" in message for message in messages))
        self.assertTrue(any("unexpected top-level" in message for message in messages))

    def test_every_public_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            public = root / "src" / "public.txt"
            public.write_text("PUBLIC", encoding="utf-8")
            (root / "src" / "alias.txt").symlink_to(public)
            (root / "src" / "directory-link").symlink_to(root / "tests")
            outside = Path(directory).parent / "outside-public-hygiene.txt"
            outside.write_text("PUBLIC", encoding="utf-8")
            try:
                (root / "src" / "escape.txt").symlink_to(outside)
                (root / "src" / "broken.txt").symlink_to(root / "missing")
                audit = hygiene.audit_repository(root)
            finally:
                outside.unlink()
        symlink_message = (
            "symbolic links are prohibited in the public source surface; replace "
            "this link with a regular file"
        )
        self.assertEqual(
            [
                issue.message
                for issue in audit.issues
                if "symbolic links are prohibited" in issue.message
            ],
            [symlink_message] * 4,
        )
        self.assertTrue(
            {"alias.txt", "broken.txt", "directory-link", "escape.txt"}.isdisjoint(
                path.name for path in audit.public_files
            )
        )

    def test_root_symlinks_and_wrong_documented_types_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            (root / "README.md").unlink()
            (root / "README.md").mkdir()
            (root / "tests").rmdir()
            (root / "tests").write_text("wrong", encoding="utf-8")
            (root / "build").write_text("wrong", encoding="utf-8")
            (root / ".coverage").mkdir()
            (root / "unexpected-link").symlink_to(root / "missing")
            audit = hygiene.audit_repository(root)
        messages = _messages(audit)
        self.assertTrue(any("documented root file" in message for message in messages))
        self.assertTrue(
            any("documented root directory" in message for message in messages)
        )
        self.assertEqual(sum("reserved generated" in item for item in messages), 2)
        self.assertTrue(
            any("symbolic links are prohibited" in item for item in messages)
        )

    def test_documented_root_file_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            target = root / "src" / "readme-public.md"
            target.write_text("PUBLIC", encoding="utf-8")
            (root / "README.md").unlink()
            (root / "README.md").symlink_to(target)

            audit = hygiene.audit_repository(root)

        self.assertNotIn(root / "README.md", audit.public_files)
        self.assertTrue(
            any("symbolic links are prohibited" in item for item in _messages(audit))
        )

    def test_unexpected_internal_symlink_reports_both_policy_failures(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            target = root / "README.md"
            (root / "unexpected-link").symlink_to(target)

            audit = hygiene.audit_repository(root)

        related = [
            issue for issue in audit.issues if issue.path.name == "unexpected-link"
        ]
        self.assertEqual(len(related), 2)
        self.assertTrue(any("unexpected top-level" in item.message for item in related))
        self.assertTrue(
            any("symbolic links are prohibited" in item.message for item in related)
        )

    def test_unexpected_escaping_symlink_reports_both_policy_failures(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            outside = root.parent / "outside-public-hygiene.txt"
            outside.write_text("PUBLIC", encoding="utf-8")
            try:
                link = root / "unexpected-link"
                link.symlink_to(outside)
                audit = hygiene.audit_repository(root)
            finally:
                outside.unlink()

        related = [issue for issue in audit.issues if issue.path == link]
        self.assertEqual(len(related), 2)
        self.assertTrue(
            any("symbolic links are prohibited" in item.message for item in related)
        )

    def test_compound_sensitive_and_key_backup_names_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            names = (
                "personal.eml.backup",
                "credentials.backup.json",
                "client_secret-production.json",
                "id_rsa.old",
                ".netrc.backup",
                "server.key.bak",
                "signing.ppk.backup",
            )
            for name in names:
                (root / "tests" / name).write_text("PRIVATE", encoding="utf-8")

            audit = hygiene.audit_repository(root)

        related_names = {issue.path.name for issue in audit.issues}
        self.assertTrue(set(names).issubset(related_names))

    def test_public_content_requires_synthetic_identity_and_portable_paths(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            public = root / "tests" / "identities.txt"
            mac_home = "/" + "Users" + "/private/Documents/mail"
            linux_home = "/" + "home" + "/private/mail"
            windows_home = "C:\\" + "Users" + "\\private\\mail"
            public.write_text(
                "public@example.test\n"
                f"private@{'example.com'}\n"
                f"{mac_home}\n"
                f"{linux_home}\n"
                f"{windows_home}\n",
                encoding="utf-8",
            )

            audit = hygiene.audit_repository(root)

        related = [issue.message for issue in audit.issues if issue.path == public]
        self.assertEqual(sum("email identity" in item for item in related), 1)
        self.assertEqual(sum("user-home path" in item for item in related), 3)

    def test_non_utf8_public_file_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            binary = root / "tests" / "private.bin"
            binary.write_bytes(b"\xff\xfe")

            audit = hygiene.audit_repository(root)

        self.assertTrue(any("not UTF-8 text" in item for item in _messages(audit)))

    def test_non_regular_public_artifact_and_kind_labels(self) -> None:
        if not hasattr(os, "mkfifo"):
            self.skipTest("os.mkfifo is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            fifo = root / "src" / "public-pipe"
            os.mkfifo(fifo)
            audit = hygiene.audit_repository(root)
        self.assertTrue(any("FIFO" in message for message in _messages(audit)))
        modes = (
            (stat.S_IFLNK, "symbolic link"),
            (stat.S_IFIFO, "FIFO"),
            (stat.S_IFSOCK, "socket"),
            (stat.S_IFCHR, "character device"),
            (stat.S_IFBLK, "block device"),
            (stat.S_IFDIR, "directory"),
            (stat.S_IFREG, "regular file"),
            (0, "unknown filesystem object"),
        )
        for mode, expected in modes:
            with self.subTest(expected=expected):
                self.assertEqual(
                    hygiene._kind(  # ruff: ignore[private-member-access]
                        mode,
                    ),
                    expected,
                )

    def test_main_reports_relative_paths_and_status(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            (root / "message.msg").write_text("PRIVATE", encoding="utf-8")
            with (
                patch.object(hygiene, "PROJECT_ROOT", root),
                patch("sys.stderr", new_callable=io.StringIO) as stderr,
            ):
                self.assertEqual(hygiene.main(), 1)
            self.assertIn("message.msg:", stderr.getvalue())
            (root / "message.msg").unlink()
            with patch.object(hygiene, "PROJECT_ROOT", root):
                self.assertEqual(hygiene.main(), 0)

    def test_main_prints_an_issue_path_outside_the_project(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            outside = root.parent / "outside-public-hygiene.txt"
            audit = hygiene.HygieneAudit(
                (),
                (hygiene.HygieneIssue(outside, "public diagnostic"),),
            )
            with (
                patch.object(hygiene, "PROJECT_ROOT", root),
                patch.object(hygiene, "audit_repository", return_value=audit),
                patch("sys.stderr", new_callable=io.StringIO) as stderr,
            ):
                self.assertEqual(hygiene.main(), 1)

        self.assertIn(str(outside), stderr.getvalue())

    def test_inspection_errors_are_reported_instead_of_aborting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = _root(directory)
            hidden = root / "src" / "hidden.py"
            hidden.write_text("PUBLIC", encoding="utf-8")
            with patch.object(Path, "lstat", side_effect=OSError("denied")):
                audit = hygiene.audit_repository(root)
            self.assertTrue(any("cannot inspect" in item for item in _messages(audit)))
            with patch.object(Path, "iterdir", side_effect=OSError("denied")):
                audit = hygiene.audit_repository(root)
            self.assertTrue(
                any("cannot list root" in item for item in _messages(audit))
            )
            original_read_bytes = Path.read_bytes

            def fail_one(path: Path) -> bytes:
                if path == hidden:
                    message = "denied"
                    raise OSError(message)
                return original_read_bytes(path)

            with patch.object(Path, "read_bytes", fail_one):
                audit = hygiene.audit_repository(root)
            self.assertTrue(
                any("cannot read public file" in item for item in _messages(audit))
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)

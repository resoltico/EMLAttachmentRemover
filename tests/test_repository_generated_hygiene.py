"""Regression tests for generated-artifact repository hygiene."""

from __future__ import annotations

import stat
import tempfile
import unittest
from os import stat_result
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import check_repository_hygiene as hygiene

from tests.mutmut_environment_support import explicit_mutmut_marker
from tests.repository_hygiene_support import issue_messages, make_public_root

if TYPE_CHECKING:
    from collections.abc import Iterator


class GeneratedArtifactHygieneTests(unittest.TestCase):
    """Exercise recursive generated-artifact inspection and isolation."""

    def test_mutmut_root_statistics_are_generated_and_content_scanned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            (parent / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
            root = parent / "mutants"
            root.mkdir()
            make_public_root(str(root))
            statistics = root / "mutmut-stats.json"
            cicd_statistics = root / "mutmut-cicd-stats.json"
            statistics.write_text('{"public": true}\n', encoding="utf-8")
            cicd_statistics.write_text('{"public": true}\n', encoding="utf-8")
            with explicit_mutmut_marker("not-a-mutmut-marker"):
                unmarked_audit = hygiene.audit_repository(root)
            with explicit_mutmut_marker(""):
                clean_audit = hygiene.audit_repository(root)
                private_home = "/" + "Users" + "/private/work"
                private_identity = "private@" + "example.com"
                statistics.write_text(
                    f'{{"path": "{private_home}", "identity": "{private_identity}"}}\n',
                    encoding="utf-8",
                )
                private_audit = hygiene.audit_repository(root)

        self.assertFalse(clean_audit.issues)
        self.assertTrue(
            any(
                issue.path == statistics and "unexpected top-level" in issue.message
                for issue in unmarked_audit.issues
            )
        )
        self.assertTrue(
            {statistics, cicd_statistics}.isdisjoint(clean_audit.public_files)
        )
        messages = [
            issue.message for issue in private_audit.issues if issue.path == statistics
        ]
        self.assertEqual(sum("user-home path" in item for item in messages), 1)
        self.assertEqual(sum("email identity" in item for item in messages), 1)

    def test_mutmut_statistics_require_a_validated_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            statistic = root / "mutmut-stats.json"
            statistic.write_text('{"public": true}\n', encoding="utf-8")
            with explicit_mutmut_marker("public.x_value__mutmut_1"):
                noncanonical_audit = hygiene.audit_repository(root)

            statistic.unlink()
            mutation_root = root / "mutants"
            mutation_root.mkdir()
            make_public_root(str(mutation_root))
            statistic = mutation_root / "mutmut-stats.json"
            statistic.write_text('{"public": true}\n', encoding="utf-8")
            project_config = root / "pyproject.toml"
            project_config.unlink()
            project_config.symlink_to(root / "README.md")
            with explicit_mutmut_marker("public.x_value__mutmut_1"):
                untrusted_audit = hygiene.audit_repository(mutation_root)

        self.assertTrue(
            any(
                issue.path == root / "mutmut-stats.json"
                and "unexpected top-level" in issue.message
                for issue in noncanonical_audit.issues
            )
        )
        self.assertTrue(
            any(
                issue.path == statistic and "unexpected top-level" in issue.message
                for issue in untrusted_audit.issues
            )
        )

    def test_mutmut_statistics_reject_nonregulars_without_reading(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            mutation_root = root / "mutants"
            mutation_root.mkdir()
            make_public_root(str(mutation_root))
            symbolic = mutation_root / "mutmut-stats.json"
            symbolic.symlink_to(mutation_root / "README.md")
            nonregular = mutation_root / "mutmut-cicd-stats.json"
            nonregular.mkdir()
            lookalike = mutation_root / "mutmut-stats.json.backup"
            lookalike.write_text('{"public": true}\n', encoding="utf-8")
            original_read_text = Path.read_text

            def reject_generated_nonregular_read(
                path: Path,
                *args: object,
                **kwargs: object,
            ) -> str:
                if path in {symbolic, nonregular}:
                    message = "forbidden generated nonregular read"
                    raise AssertionError(message)
                return original_read_text(path, *args, **kwargs)  # type: ignore[arg-type]

            with (
                explicit_mutmut_marker("public.x_value__mutmut_1"),
                patch.object(Path, "read_text", reject_generated_nonregular_read),
            ):
                symbolic_audit = hygiene.audit_repository(mutation_root)

        symbolic_messages = [
            issue.message for issue in symbolic_audit.issues if issue.path == symbolic
        ]
        self.assertTrue(any("reserved generated" in item for item in symbolic_messages))
        self.assertTrue(any("symbolic links" in item for item in symbolic_messages))
        self.assertTrue(
            any(
                issue.path == nonregular and "reserved generated" in issue.message
                for issue in symbolic_audit.issues
            )
        )
        self.assertTrue(
            any(
                issue.path == lookalike and "unexpected top-level" in issue.message
                for issue in symbolic_audit.issues
            )
        )

    def test_mutmut_sidecars_require_a_paired_source_and_canonical_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            (parent / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
            root = parent / "mutants"
            root.mkdir()
            make_public_root(str(root))
            source = root / "src" / "public.py"
            source.write_text("PUBLIC = True\n", encoding="utf-8")
            paired = root / "src" / "public.py.meta"
            paired.write_text('{"public": true}\n', encoding="utf-8")
            unpaired = root / "src" / "missing.py.spans"
            unpaired.write_text('{"public": true}\n', encoding="utf-8")
            with explicit_mutmut_marker("public.x_value__mutmut_1"):
                canonical = hygiene.audit_repository(root)
            with explicit_mutmut_marker("not-a-mutmut-marker"):
                decoy = hygiene.audit_repository(root)

        self.assertNotIn(paired, canonical.public_files)
        self.assertIn(unpaired, canonical.public_files)
        self.assertIn(paired, decoy.public_files)

    def test_generated_roots_are_recursively_checked_for_private_names(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            private_paths = (
                root / "build" / "nested" / "personal.eml",
                root / "dist" / ".env.production",
                root / "mutants" / "id_rsa",
                root / "release-dist" / "signing-key.pem",
                root / ".hypothesis" / "examples" / "inbox.pst",
                root / "src" / "__pycache__" / "message.eml.pyc",
                root / ".coverage.personal.eml",
            )
            for path in private_paths:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("PRIVATE", encoding="utf-8")
            target = root / "README.md"
            generated_link = root / "build" / "public-link.txt"
            generated_link.symlink_to(target)

            audit = hygiene.audit_repository(root)

        related_paths = {issue.path for issue in audit.issues}
        self.assertTrue(set(private_paths).issubset(related_paths))
        self.assertTrue(set(private_paths).isdisjoint(audit.public_files))
        self.assertIn(generated_link, related_paths)

    def test_generated_utf8_evidence_cannot_retain_private_machine_data(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            private_log = root / "build" / "test.log"
            private_log.parent.mkdir()
            private_home = "/" + "Users" + "/private/workspace"
            private_hostname = "local-machine.invalid"
            private_log.write_text(
                f"{private_home}\nprivate@{'example.com'}\n{private_hostname}\n",
                encoding="utf-8",
            )
            binary = root / "build" / "coverage.db"
            binary.write_bytes(b"\xff\xfe")

            with patch(
                "tools.repository_hygiene_policy.socket.gethostname",
                return_value=private_hostname,
            ):
                audit = hygiene.audit_repository(root)

        messages = [
            issue.message for issue in audit.issues if issue.path == private_log
        ]
        self.assertEqual(sum("user-home path" in item for item in messages), 1)
        self.assertEqual(sum("email identity" in item for item in messages), 1)
        self.assertEqual(sum("local hostname" in item for item in messages), 1)
        self.assertEqual(
            [issue.message for issue in audit.issues if issue.path == binary],
            [
                (
                    "opaque generated file cannot be audited for private machine "
                    "data; remove it or publish it only as an explicitly validated "
                    "build/release artifact"
                )
            ],
        )

    def test_cache_roots_and_opaque_outputs_fail_closed_but_archives_are_explicit(
        self,
    ) -> None:
        """Reject local caches while preserving only shaped release artifacts."""
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            cache_roots = tuple(
                root / name
                for name in sorted(hygiene.policy.TOOL_CACHE_DIRECTORY_NAMES)
            )
            for cache_root in cache_roots:
                cache_root.mkdir()
            bytecode_root = root / "tools" / "__pycache__"
            bytecode_root.mkdir()
            bytecode = bytecode_root / "public.cpython-314.pyc"
            bytecode.write_bytes(b"\xff/private-machine-path")
            coverage = root / ".coverage"
            coverage.write_bytes(b"\xff/private-machine-path")
            opaque_mutant = root / "mutants" / "instrumented.pyz"
            opaque_mutant.parent.mkdir()
            opaque_mutant.write_bytes(b"\xff/private-machine-path")
            expected_archives = (
                root / "build" / "remove-eml-attachments.pyz",
                root / "dist" / "public-1.0.0.tar.gz",
                root / "release-dist" / "public-1.0.0-cp314-none-any.whl",
            )
            for archive in expected_archives:
                archive.parent.mkdir()
                archive.write_bytes(b"\xffvalidated archive")
            opaque_lookalike = root / "build" / "nested" / "private.pyz"
            opaque_lookalike.parent.mkdir()
            opaque_lookalike.write_bytes(b"\xffnot validated")
            venv_bytecode = root / ".venv" / "lib" / "private.pyc"
            venv_bytecode.parent.mkdir(parents=True)
            venv_bytecode.write_bytes(b"\xffopaque environment")

            audit = hygiene.audit_repository(root)

        issue_paths = {issue.path for issue in audit.issues}
        self.assertTrue(set(cache_roots).issubset(issue_paths))
        self.assertIn(bytecode_root, issue_paths)
        self.assertIn(bytecode, issue_paths)
        self.assertIn(coverage, issue_paths)
        self.assertIn(opaque_mutant, issue_paths)
        self.assertIn(opaque_lookalike, issue_paths)
        self.assertTrue(set(expected_archives).isdisjoint(issue_paths))
        self.assertNotIn(venv_bytecode, issue_paths)

    def test_generated_directory_errors_and_child_inspection_errors_are_issues(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = make_public_root(directory)
            opaque = root / ".venv"
            opaque.mkdir()
            (opaque / "personal.eml").write_text("PRIVATE", encoding="utf-8")
            generated = root / "build"
            generated.mkdir()
            nested_venv_mail = generated / ".venv" / "nested.eml"
            nested_git_key = generated / ".git" / "id_rsa"
            for private_path in (nested_venv_mail, nested_git_key):
                private_path.parent.mkdir()
                private_path.write_text("PRIVATE", encoding="utf-8")
            blocked = generated / "blocked.txt"
            blocked.write_text("PUBLIC", encoding="utf-8")
            unsupported = generated / "unsupported.txt"
            unsupported.write_text("PUBLIC", encoding="utf-8")
            original_iterdir = Path.iterdir
            original_lstat = Path.lstat

            def fail_generated(path: Path) -> Iterator[Path]:
                if path == generated:
                    message = "denied"
                    raise OSError(message)
                return original_iterdir(path)

            with patch.object(Path, "iterdir", fail_generated):
                listing_audit = hygiene.audit_repository(root)

            def fail_child(path: Path) -> stat_result:
                if path == blocked:
                    message = "denied"
                    raise OSError(message)
                if path == unsupported:
                    return stat_result((stat.S_IFIFO, 0, 0, 0, 0, 0, 0, 0, 0, 0))
                return original_lstat(path)

            with patch.object(Path, "lstat", fail_child):
                child_audit = hygiene.audit_repository(root)

            with patch.object(Path, "read_bytes", side_effect=OSError("denied")):
                read_audit = hygiene.audit_repository(root)

        self.assertTrue(
            any(
                "cannot list generated directory" in item
                for item in issue_messages(listing_audit)
            )
        )
        self.assertTrue(
            any("cannot inspect" in item for item in issue_messages(child_audit))
        )
        self.assertFalse(
            any(issue.path.name == "personal.eml" for issue in child_audit.issues)
        )
        self.assertTrue(
            {nested_venv_mail, nested_git_key}.issubset(
                issue.path for issue in child_audit.issues
            )
        )
        self.assertTrue(
            any(
                "unsupported generated artifact" in item
                for item in issue_messages(child_audit)
            )
        )
        self.assertTrue(
            any(
                "cannot read generated file" in item
                for item in issue_messages(read_audit)
            )
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)

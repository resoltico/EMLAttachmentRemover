"""Verify static quality checks and their fresh source-hygiene boundary."""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

from tools import tasks


class StaticCheckTests(unittest.TestCase):
    """Exercise static commands and the fresh audit before secret scanning."""

    def test_check_runs_every_static_gate(self) -> None:
        public_files = (Path("/public/README.md"), Path("/public/src/public.py"))
        audit = MagicMock(issues=(), public_files=public_files)
        with (
            patch.object(tasks, "_run") as run,
            patch.object(
                tasks.check_repository_hygiene,
                "audit_repository",
                return_value=audit,
            ),
        ):
            self.assertEqual(tasks.main(["check"]), 0)
        self.assertEqual(run.call_count, 11)
        self.assertEqual(
            run.call_args_list[0],
            call((sys.executable, "tools/check_repository_hygiene.py")),
        )
        self.assertEqual(
            run.call_args_list[1],
            call(("ruff", "format", "--check", "--no-cache", ".")),
        )
        self.assertEqual(
            run.call_args_list[2], call(("ruff", "check", "--no-cache", "."))
        )
        self.assertEqual(
            run.call_args_list[3],
            call((
                "mypy",
                "--no-incremental",
                "--cache-dir",
                os.devnull,
                "--platform",
                "linux",
            )),
        )
        self.assertEqual(
            run.call_args_list[6],
            call((sys.executable, "tools/check_module_design.py")),
        )
        actionlint_command = run.call_args_list[8].args[0]
        self.assertEqual(
            actionlint_command,
            (
                "actionlint",
                "-shellcheck",
                "shellcheck",
                "-pyflakes",
                "pyflakes",
                "-config-file",
                str(tasks.PROJECT_ROOT / ".github/actionlint.yaml"),
                *tasks.WORKFLOW_FILES,
            ),
        )
        self.assertEqual(
            run.call_args_list[9],
            call(("shellcheck", "--shell=sh", *tasks.SHELL_SCRIPTS)),
        )
        secret_command = run.call_args_list[10].args[0]
        self.assertEqual(secret_command[:2], ("detect-secrets-hook", "--no-verify"))
        self.assertTrue(
            all(str(path) in secret_command for path in public_files),
        )

    def test_check_preserves_multiline_dirty_workspace_diagnostics(self) -> None:
        audit = MagicMock(issues=(object(),), public_files=())
        audit.diagnostics.return_value = (
            "alpha.txt: first issue",
            "beta.txt: second issue",
        )
        with (
            patch.object(tasks, "_run"),
            patch.object(
                tasks.check_repository_hygiene,
                "audit_repository",
                return_value=audit,
            ),
            self.assertRaises(RuntimeError) as raised,
        ):
            tasks.main(["check"])
        self.assertEqual(
            str(raised.exception),
            "repository changed before secret scanning:\n"
            "alpha.txt: first issue\n"
            "beta.txt: second issue",
        )
        audit.diagnostics.assert_called_once_with(tasks.PROJECT_ROOT)

    def test_check_rejects_a_workspace_dirtied_after_initial_hygiene(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()

            def dirty_after_first_gate(*_args: object, **_kwargs: object) -> None:
                (root / "late-public.txt").write_text("PUBLIC\n", encoding="utf-8")

            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "_run", side_effect=dirty_after_first_gate) as run,
                self.assertRaisesRegex(
                    RuntimeError,
                    r"repository changed before secret scanning:\n"
                    r"late-public\.txt: unexpected top-level regular file",
                ),
            ):
                tasks.main(["check"])
        self.assertEqual(run.call_count, 10)
        self.assertFalse(
            any(
                call_args.args[0][0] == "detect-secrets-hook"
                for call_args in run.call_args_list
            ),
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)

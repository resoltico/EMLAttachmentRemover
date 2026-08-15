"""Focused regressions for branch-level reporting and tooling contracts."""

from __future__ import annotations

import ast
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from tools import check_module_design, tasks

from eml_attachment_remover import models, reporting


class ReportingBranchTests(unittest.TestCase):
    """Exercise report transitions that have distinct visible output."""

    def test_non_dry_result_without_output_writes_only_its_warning(self) -> None:
        result = models.ProcessResult(
            source=Path("public.eml"),
            destination=None,
            source_size=1,
            output_size=None,
            removed=(),
            preserved_file_parts=(),
            warnings=("public warning",),
            dry_run=False,
        )
        stream = io.StringIO()
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_result(stream, result)

        self.assertIn(
            call(stream, "Warning: public warning"), write_line.call_args_list
        )
        self.assertFalse(
            any(
                arguments.args[1].startswith(("Wrote:", "Size:"))
                for arguments in write_line.call_args_list
            ),
        )

    def test_human_batch_separates_a_result_from_a_following_skip(self) -> None:
        result = models.ProcessResult(
            source=Path("public.eml"),
            destination=Path("output.eml"),
            source_size=1,
            output_size=1,
            removed=(),
            preserved_file_parts=(),
            warnings=(),
            dry_run=False,
        )
        skip = models.BatchSkip(Path("second.eml"), Path("second-output.eml"))
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_human_batch([result], [skip], [], multiple=False)

        skip_call = call(
            sys.stdout,
            "Skipped existing output for second.eml: second-output.eml",
        )
        skip_index = write_line.call_args_list.index(skip_call)
        self.assertEqual(
            write_line.call_args_list[skip_index - 1], call(sys.stdout, "")
        )


class ToolBranchTests(unittest.TestCase):
    """Exercise structural and task-runner decision boundaries."""

    def test_acceptable_class_has_no_design_violation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            module = Path(directory) / "acceptable.py"
            module.write_text(
                "class Public:\n    def method(self):\n        return None\n",
                encoding="utf-8",
            )
            tree = ast.parse(module.read_text(encoding="utf-8"))

            self.assertIsInstance(tree.body[0], ast.ClassDef)
            self.assertEqual(check_module_design._module_violations(module), [])

    def test_run_without_profile_does_not_invent_one(self) -> None:
        with (
            patch("tools.tasks.os.environ.copy", return_value={}),
            patch("tools.tasks.subprocess.run") as run,
        ):
            tasks._run(("public-command",))

        self.assertNotIn("HYPOTHESIS_PROFILE", run.call_args.kwargs["env"])

"""Behavioral tests for the repository module-design budget."""

from __future__ import annotations

import ast
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import check_module_design


class ModuleDesignToolTests(unittest.TestCase):
    """Exercise every structural-budget outcome."""

    def test_line_and_class_counters(self) -> None:
        self.assertEqual(check_module_design._line_count(""), 0)
        self.assertEqual(check_module_design._line_count("public"), 1)
        self.assertEqual(check_module_design._line_count("public\n"), 1)
        self.assertEqual(
            check_module_design._substantive_line_count(
                "# comment\nPUBLIC = 1  # inline comment\n\n'content'\n",
            ),
            2,
        )
        tree = ast.parse("class Public:\n    def method(self):\n        return None\n")
        class_node = tree.body[0]
        assert isinstance(class_node, ast.ClassDef)
        self.assertEqual(check_module_design._class_method_count(class_node), 1)

        for source, expected in (
            ("# first\nvalue = 1\n", 1),
            ("value = 1\n# middle\nother = 2\n", 2),
            ("value = 1\n  # indented comment\nother = 2\n", 2),
            ("value = 1\n# last\n", 1),
            ("value = 1  # inline\n", 1),
        ):
            with self.subTest(source=source):
                self.assertEqual(
                    check_module_design._substantive_line_count(source),
                    expected,
                )

    def test_exact_budget_boundaries_and_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            physical = base / "physical.py"
            physical.write_text(
                "VALUE = 1\n" + "\n" * (check_module_design.MAX_MODULE_LINES - 1),
                encoding="utf-8",
            )
            self.assertEqual(check_module_design._module_violations(physical), [])
            physical.write_text(
                "VALUE = 1\n" + "\n" * check_module_design.MAX_MODULE_LINES,
                encoding="utf-8",
            )
            self.assertEqual(
                check_module_design._module_violations(physical),
                [
                    check_module_design.DesignViolation(
                        physical,
                        "has 451 lines; limit is 450",
                    )
                ],
            )

            dense = base / "dense.py"
            dense.write_text(
                "VALUE = 1\n" * check_module_design.MAX_SUBSTANTIVE_LINES,
                encoding="utf-8",
            )
            self.assertEqual(check_module_design._module_violations(dense), [])
            dense.write_text(
                "VALUE = 1\n" * (check_module_design.MAX_SUBSTANTIVE_LINES + 1),
                encoding="utf-8",
            )
            self.assertEqual(
                check_module_design._module_violations(dense),
                [
                    check_module_design.DesignViolation(
                        dense,
                        "has 401 substantive lines; limit is 400",
                    )
                ],
            )

            declarations = base / "declarations.py"
            declarations.write_text(
                "".join(
                    f"def public_{index}():\n    pass\n"
                    for index in range(check_module_design.MAX_TOP_LEVEL_DECLARATIONS)
                ),
                encoding="utf-8",
            )
            self.assertEqual(check_module_design._module_violations(declarations), [])
            declarations.write_text(
                declarations.read_text(encoding="utf-8") + "def excess():\n    pass\n",
                encoding="utf-8",
            )
            self.assertEqual(
                check_module_design._module_violations(declarations),
                [
                    check_module_design.DesignViolation(
                        declarations,
                        "has 21 top-level declarations; limit is 20",
                    )
                ],
            )

            methods = base / "methods.py"
            method_source = "class Public:\n" + "".join(
                f"    def method_{index}(self):\n        pass\n"
                for index in range(check_module_design.MAX_CLASS_METHODS)
            )
            methods.write_text(method_source, encoding="utf-8")
            self.assertEqual(check_module_design._module_violations(methods), [])
            methods.write_text(
                method_source + "    def excess(self):\n        pass\n",
                encoding="utf-8",
            )
            self.assertEqual(
                check_module_design._module_violations(methods),
                [
                    check_module_design.DesignViolation(
                        methods,
                        "class Public has 21 methods; limit is 20",
                    )
                ],
            )

    def test_source_read_and_syntax_errors_retain_portable_context(self) -> None:
        path = Path("/public/module.py")
        with patch.object(
            Path,
            "read_text",
            return_value="VALUE = 1\n",
        ) as read_text:
            self.assertEqual(check_module_design._module_violations(path), [])
        read_text.assert_called_once_with(encoding="utf-8")

        with tempfile.TemporaryDirectory() as directory:
            malformed = Path(directory) / "malformed.py"
            malformed.write_text("if:\n", encoding="utf-8")
            with self.assertRaises(SyntaxError) as raised:
                check_module_design._module_violations(malformed)
        self.assertEqual(raised.exception.filename, str(malformed))

    def test_module_violation_categories(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            acceptable = base / "acceptable.py"
            acceptable.write_text("VALUE = 1\n", encoding="utf-8")
            self.assertEqual(check_module_design._module_violations(acceptable), [])

            too_long = base / "too_long.py"
            too_long.write_text("\n" * 500, encoding="utf-8")
            self.assertIn(
                "lines",
                check_module_design._module_violations(too_long)[0].message,
            )

            too_dense = base / "too_dense.py"
            too_dense.write_text("VALUE = 1\n" * 401, encoding="utf-8")
            self.assertTrue(
                any(
                    "substantive lines" in violation.message
                    for violation in check_module_design._module_violations(too_dense)
                ),
            )

            declarations = base / "declarations.py"
            declarations.write_text(
                "".join(f"def public_{index}():\n    pass\n" for index in range(21)),
                encoding="utf-8",
            )
            self.assertTrue(
                any(
                    "top-level declarations" in violation.message
                    for violation in check_module_design._module_violations(
                        declarations
                    )
                ),
            )

            methods = base / "methods.py"
            methods.write_text(
                "class Public:\n"
                + "".join(
                    f"    def method_{index}(self):\n        pass\n"
                    for index in range(21)
                ),
                encoding="utf-8",
            )
            self.assertTrue(
                any(
                    "class Public" in violation.message
                    for violation in check_module_design._module_violations(methods)
                ),
            )

    def test_file_discovery_and_main_status(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in check_module_design.SCANNED_DIRECTORIES:
                target = root / name
                target.mkdir()
                (target / f"{name}.py").write_text("VALUE = 1\n", encoding="utf-8")
            with patch.object(check_module_design, "PROJECT_ROOT", root):
                self.assertEqual(len(tuple(check_module_design._python_files())), 3)
                self.assertEqual(check_module_design.main(), 0)
                (root / "tools" / "tools.py").write_text("\n" * 500, encoding="utf-8")
                with patch("sys.stderr", new_callable=io.StringIO) as stderr:
                    self.assertEqual(check_module_design.main(), 1)
                self.assertIn("tools/tools.py", stderr.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)

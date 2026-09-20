"""Contracts for parallel Mutmut pytest temporary-directory isolation."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from typing import Any
from unittest.mock import patch

from tools import mutation_pytest_isolation

from tests.mutmut_environment_support import selector_preserving_environment


class MutationPytestIsolationTests(unittest.TestCase):
    """Verify the early pytest hook adds only a private process directory."""

    def test_uses_process_scoped_base_temp_when_enabled(self) -> None:
        args = ["-q"]
        plugin_argument: Any = object()
        with (
            patch.dict(
                os.environ,
                selector_preserving_environment({
                    mutation_pytest_isolation.TEMPORARY_ROOT_VARIABLE: "/private/root"
                }),
                clear=True,
            ),
            patch.object(os, "getpid", return_value=17),
        ):
            mutation_pytest_isolation.pytest_load_initial_conftests(
                plugin_argument, plugin_argument, args
            )
        self.assertEqual(
            args,
            ["-q", "--basetemp", str(Path("/private/root") / "17")],
        )
        self._assert_generated_workspace_hook_is_loaded()

    def test_leaves_arguments_unchanged_when_not_enabled(self) -> None:
        args = ["-q"]
        plugin_argument: Any = object()
        with patch.dict(
            os.environ,
            selector_preserving_environment({}),
            clear=True,
        ):
            mutation_pytest_isolation.pytest_load_initial_conftests(
                plugin_argument, plugin_argument, args
            )
        self.assertEqual(args, ["-q"])

    def test_workspace_lookup_and_marker_without_workspace_leave_imports_unchanged(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertIsNone(mutation_pytest_isolation._workspace_import_root(root))  # ruff: ignore[private-member-access] - absence contract.
            args: list[str] = []
            plugin_argument: Any = object()
            with (
                patch.dict(
                    os.environ,
                    {mutation_pytest_isolation.MUTANT_MARKER_VARIABLE: "x__mutmut_1"},
                ),
                patch.object(sys, "path", []),
                patch.object(sys.modules["tools"], "__path__", []),
            ):
                with patch.object(Path, "cwd", return_value=root):
                    self.assertIs(
                        sys.modules["tools"],
                        sys.modules[mutation_pytest_isolation.__package__],
                    )
                    mutation_pytest_isolation.pytest_load_initial_conftests(
                        plugin_argument, plugin_argument, args
                    )
                self.assertEqual(sys.path, [])

    def test_workspace_lookup_requires_both_repository_markers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tools").mkdir()
            self.assertIsNone(mutation_pytest_isolation._workspace_import_root(root))  # ruff: ignore[private-member-access] - incomplete workspace contract.
            (root / "tools").rmdir()
            (root / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
            self.assertIsNone(mutation_pytest_isolation._workspace_import_root(root))  # ruff: ignore[private-member-access] - incomplete workspace contract.

    def _assert_generated_workspace_hook_is_loaded(self) -> None:
        current = Path.cwd()
        candidate = current / "mutants"
        workspace = (
            candidate
            if (candidate / "pyproject.toml").is_file()
            and (candidate / "tools").is_dir()
            else current
        )
        generated = workspace / "tools" / "mutation_pytest_isolation.py"
        if not generated.is_file():
            self.skipTest("mutation hook source is unavailable")
        virtual_environment = Path(os.environ.get("VIRTUAL_ENV", sys.prefix))
        interpreter = virtual_environment / (
            "Scripts/python.exe" if os.name == "nt" else "bin/python"
        )
        environment = os.environ.copy()
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        environment.setdefault(
            mutation_pytest_isolation.MUTANT_MARKER_VARIABLE, "probe"
        )
        environment["EML_MUTATION_PROBE_HOOK_PATH"] = str(generated)
        result = subprocess.run(
            [
                str(interpreter),
                "-c",
                textwrap.dedent(
                    """
                    import importlib.util
                    import os
                    import sys
                    from pathlib import Path

                    import tools

                    spec = importlib.util.spec_from_file_location(
                        "tools.mutation_pytest_isolation",
                        Path(os.environ["EML_MUTATION_PROBE_HOOK_PATH"]),
                    )
                    assert spec is not None
                    assert spec.loader is not None
                    hook = importlib.util.module_from_spec(spec)
                    sys.modules[spec.name] = hook
                    spec.loader.exec_module(hook)

                    args = []
                    hook.pytest_load_initial_conftests(None, None, args)
                    print(hook.__file__)
                    print(hook._workspace_import_root(hook.Path.cwd()))
                    print(sys.path[0])
                    print(tools.__path__[0])

                    class ProbePath:
                        def __init__(self, parts):
                            self.parts = parts

                        def __truediv__(self, part):
                            return ProbePath(self.parts + (part,))

                        def is_dir(self):
                            return self.parts in {
                                ("root",),
                                ("root", "mutants"),
                                ("root", "mutants", "tools"),
                            }

                        def is_file(self):
                            return self.parts == ("root", "mutants", "pyproject.toml")

                        def __str__(self):
                            return "/".join(self.parts)

                    print(hook._workspace_import_root(ProbePath(("root",))))

                    tools.__path__ = None
                    hook.pytest_load_initial_conftests(None, None, [])
                    print(sys.path[0])
                    print(tools.__path__)
                    """
                ),
            ],
            cwd=workspace.parent if workspace.name == "mutants" else workspace,
            capture_output=True,
            check=True,
            env=environment,
            text=True,
        )
        self.assertEqual(
            result.stdout.splitlines(),
            [
                str(workspace / "tools" / "mutation_pytest_isolation.py"),
                str(workspace),
                str(workspace),
                str(workspace / "tools"),
                "root/mutants",
                str(workspace),
                "None",
            ],
        )

    def test_generated_workspace_hook_is_loaded_in_a_fresh_process(self) -> None:
        self._assert_generated_workspace_hook_is_loaded()

    def test_mutant_marker_shadows_an_editable_live_checkout_with_workspace(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "mutants"
            (workspace / "tools").mkdir(parents=True)
            (workspace / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
            self.assertEqual(
                mutation_pytest_isolation._workspace_import_root(root),  # ruff: ignore[private-member-access] - exact workspace selection contract.
                workspace,
            )
            args: list[str] = []
            plugin_argument: Any = object()
            with (
                patch.dict(
                    os.environ,
                    {mutation_pytest_isolation.MUTANT_MARKER_VARIABLE: "x__mutmut_1"},
                ),
                patch.object(sys, "path", []),
                patch.object(sys.modules["tools"], "__path__", []),
            ):
                with patch.object(Path, "cwd", return_value=root):
                    mutation_pytest_isolation.pytest_load_initial_conftests(
                        plugin_argument, plugin_argument, args
                    )
                self.assertEqual(sys.path, [str(workspace)])
                self.assertEqual(
                    sys.modules["tools"].__path__, [str(workspace / "tools")]
                )
                with patch.object(Path, "cwd", return_value=root):
                    mutation_pytest_isolation.pytest_load_initial_conftests(
                        plugin_argument, plugin_argument, args
                    )
                self.assertEqual(
                    sys.modules["tools"].__path__, [str(workspace / "tools")]
                )

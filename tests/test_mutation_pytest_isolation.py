"""Contracts for parallel Mutmut pytest temporary-directory isolation."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, Any
from unittest.mock import patch

import pytest
from tools import mutation_pytest_isolation

from tests.mutmut_environment_support import selector_preserving_environment

if TYPE_CHECKING:
    from collections.abc import Iterator


@contextmanager
def _marker_environment(marker: str) -> Iterator[None]:
    """Set a mutant marker with basetemp isolation off, as outside Mutmut.

    Yields:
        Control while the environment is patched; it is restored afterwards.

    """
    with patch.dict(
        os.environ, {mutation_pytest_isolation.MUTANT_MARKER_VARIABLE: marker}
    ):
        # A mutation campaign sets this for the whole suite; these tests exercise
        # import selection only, with no pytest configuration to record into.
        os.environ.pop(mutation_pytest_isolation.TEMPORARY_ROOT_VARIABLE, None)
        yield


class MutationPytestIsolationTests(unittest.TestCase):
    """Verify the early pytest hook adds only a private process directory."""

    def test_uses_process_scoped_base_temp_when_enabled(self) -> None:
        args = ["-q"]
        plugin_argument: Any = object()
        config: Any = SimpleNamespace(stash=pytest.Stash())
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
                config, plugin_argument, args
            )
        self.assertEqual(
            args,
            ["-q", "--basetemp", str(Path("/private/root") / "17")],
        )
        self.assertEqual(
            config.stash[mutation_pytest_isolation.BASETEMP_KEY],
            (Path("/private/root") / "17", 17),
        )
        self._assert_generated_workspace_hook_is_loaded()

    def test_unconfigure_removes_only_the_creating_process_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            basetemp = Path(directory) / "17"
            (basetemp / "nested").mkdir(parents=True)
            config: Any = SimpleNamespace(stash=pytest.Stash())
            config.stash[mutation_pytest_isolation.BASETEMP_KEY] = (basetemp, 17)
            with patch.object(os, "getpid", return_value=18):
                mutation_pytest_isolation.pytest_unconfigure(config)
            self.assertTrue((basetemp / "nested").is_dir())
            with patch.object(os, "getpid", return_value=17):
                mutation_pytest_isolation.pytest_unconfigure(config)
                self.assertFalse(basetemp.exists())
                # A directory that is already gone must never fail the session.
                mutation_pytest_isolation.pytest_unconfigure(config)
            unrecorded: Any = SimpleNamespace(stash=pytest.Stash())
            mutation_pytest_isolation.pytest_unconfigure(unrecorded)

    def test_completed_real_sessions_leave_no_private_directories(self) -> None:
        project = Path(mutation_pytest_isolation.__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "root"
            suite = base / "suite"
            root.mkdir()
            suite.mkdir()
            # Its own configuration keeps the probe independent of this repository's.
            (suite / "pytest.ini").write_text("[pytest]\n", encoding="utf-8")
            (suite / "test_probe.py").write_text(
                "def test_probe(tmp_path):\n"
                "    (tmp_path / 'fixture').write_bytes(bytes(32768))\n",
                encoding="utf-8",
            )
            environment = os.environ.copy()
            environment["PYTHONPATH"] = str(project)
            environment[mutation_pytest_isolation.TEMPORARY_ROOT_VARIABLE] = str(root)
            command = (
                sys.executable,
                "-m",
                "pytest",
                "-q",
                "-p",
                "no:cacheprovider",
                "-p",
                "tools.mutation_pytest_isolation",
                str(suite),
            )
            for _session in range(3):
                # The project root is the cwd so Mutmut's copied plugin finds its
                # configuration when this runs inside a mutation workspace.
                completed = subprocess.run(
                    command,
                    cwd=project,
                    env=environment,
                    check=False,
                    capture_output=True,
                    text=True,
                    timeout=120,
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertEqual(list(root.iterdir()), [])

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
                _marker_environment("x__mutmut_1"),
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
        # This probe covers import selection only; it passes no pytest config.
        environment.pop(mutation_pytest_isolation.TEMPORARY_ROOT_VARIABLE, None)
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
                _marker_environment("x__mutmut_1"),
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

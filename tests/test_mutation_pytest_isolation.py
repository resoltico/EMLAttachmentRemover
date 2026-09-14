"""Contracts for parallel Mutmut pytest temporary-directory isolation."""

from __future__ import annotations

import os
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

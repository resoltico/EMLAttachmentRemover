"""Contracts for parallel Mutmut pytest temporary-directory isolation."""

from __future__ import annotations

import os
import unittest
from typing import Any
from unittest.mock import patch

from tools import mutation_pytest_isolation


class MutationPytestIsolationTests(unittest.TestCase):
    """Verify the early pytest hook adds only a private process directory."""

    def test_uses_process_scoped_base_temp_when_enabled(self) -> None:
        args = ["-q"]
        plugin_argument: Any = object()
        with (
            patch.object(os.environ, "get", return_value="/private/root"),
            patch.object(os, "getpid", return_value=17),
        ):
            mutation_pytest_isolation.pytest_load_initial_conftests(
                plugin_argument, plugin_argument, args
            )
        self.assertEqual(args, ["-q", "--basetemp", "/private/root/17"])

    def test_leaves_arguments_unchanged_when_not_enabled(self) -> None:
        args = ["-q"]
        plugin_argument: Any = object()
        with patch.object(os.environ, "get", return_value=None):
            mutation_pytest_isolation.pytest_load_initial_conftests(
                plugin_argument, plugin_argument, args
            )
        self.assertEqual(args, ["-q"])

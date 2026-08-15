"""Prevent tests from hiding Mutmut's active trampoline selector."""

from __future__ import annotations

import ast
import os
import unittest
from pathlib import Path
from typing import Final
from unittest.mock import patch

from tools import mutmut_workspace

from tests.mutmut_environment_support import selector_preserving_environment
from tests.test_support import subprocess_environment

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]


def _name(node: ast.AST) -> str:
    """Return a dotted expression name when statically available.

    Returns:
        The dotted attribute name, or an empty string for another expression.

    """
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        parent = _name(node.value)
        return f"{parent}.{node.attr}" if parent else node.attr
    return ""


def _contains_selector(node: ast.AST) -> bool:
    """Return whether an expression names Mutmut's selector.

    Returns:
        Whether the expression contains the literal or shared marker constant.

    """
    return any(
        (isinstance(child, ast.Constant) and child.value == mutmut_workspace.MARKER)
        or _name(child).endswith(".MARKER")
        for child in ast.walk(node)
    )


def _unsafe_environment_operations(path: Path, source: str) -> tuple[str, ...]:
    """Inventory environment operations capable of masking a Mutmut selector.

    Returns:
        Stable line-qualified descriptions of unsafe operations.

    """
    tree = ast.parse(source, filename=str(path))
    issues: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            issues.extend(_unsafe_call(path, node))
        elif isinstance(
            node,
            (ast.Assign, ast.AnnAssign, ast.AugAssign, ast.Delete),
        ) and _contains_environment_selector_target(node):
            issues.append(f"{path.name}:{node.lineno}:direct selector mutation")
    return tuple(issues)


def _unsafe_call(path: Path, call: ast.Call) -> tuple[str, ...]:
    """Return an issue for one selector-masking environment call.

    Returns:
        Zero or one line-qualified issue.

    """
    call_name = _name(call.func)
    call_owner = _name(call.func.value) if isinstance(call.func, ast.Attribute) else ""
    issue: str | None = None
    if call_name.endswith((".setenv", ".delenv")) and any(
        _contains_selector(argument) for argument in call.args[:1]
    ):
        issue = "direct selector mutation"
    elif call_name.endswith(".clear") and call_owner.endswith("environ"):
        issue = "environment clear"
    elif (
        call_name.endswith((".pop", ".update"))
        and call_owner.endswith("environ")
        and _contains_selector(call)
    ):
        issue = "direct selector mutation"
    elif call_name.endswith("patch.dict") and len(call.args) >= 2:
        issue = _unsafe_patch_dict(call)
    return () if issue is None else (f"{path.name}:{call.lineno}:{issue}",)


def _unsafe_patch_dict(call: ast.Call) -> str | None:
    """Return the masking defect in one environment patch, if any.

    Returns:
        A stable defect name, or ``None`` for a selector-safe patch.

    """
    target = call.args[0]
    targets_environment = _name(target).endswith("environ") or (
        isinstance(target, ast.Constant) and target.value == "os.environ"
    )
    if not targets_environment:
        return None
    if _contains_selector(call.args[1]):
        return "direct selector patch"
    clear = next(
        (keyword.value for keyword in call.keywords if keyword.arg == "clear"),
        None,
    )
    mapping = call.args[1]
    preserving = isinstance(mapping, ast.Call) and _name(mapping.func).endswith(
        "selector_preserving_environment"
    )
    if isinstance(clear, ast.Constant) and clear.value is True and not preserving:
        return "unprotected environment clear"
    return None


def _contains_environment_selector_target(node: ast.AST) -> bool:
    """Return whether an assignment or deletion targets the selector.

    Returns:
        Whether a direct ``os.environ[MARKER]`` target is present.

    """
    for child in ast.walk(node):
        if not isinstance(child, ast.Subscript):
            continue
        if _name(child.value).endswith("environ") and _contains_selector(child.slice):
            return True
    return False


class MutmutEnvironmentSafetyTests(unittest.TestCase):
    """Require selector-safe test isolation and child environments."""

    def test_test_tree_has_no_selector_masking_environment_operations(self) -> None:
        issues = tuple(
            issue
            for path in sorted((PROJECT_ROOT / "tests").rglob("*.py"))
            for issue in _unsafe_environment_operations(
                path.relative_to(PROJECT_ROOT),
                path.read_text(encoding="utf-8"),
            )
        )
        self.assertEqual(issues, ())

    def test_guard_detects_clear_delete_and_overwrite_patterns(self) -> None:
        source = (
            "patch.dict(os.environ, {}, clear=True)\n"
            "monkeypatch.delenv('MUTANT_UNDER_TEST')\n"
            "os.environ['MUTANT_UNDER_TEST'] = 'stats'\n"
        )
        self.assertEqual(
            len(_unsafe_environment_operations(Path("synthetic.py"), source)),
            3,
        )

    def test_controlled_and_subprocess_environments_preserve_the_selector(self) -> None:
        selector = "package.x_value__mutmut_1"
        with patch.object(os.environ, "get", return_value=selector):
            controlled = selector_preserving_environment({"PUBLIC": "value"})
        with patch.object(
            os.environ,
            "copy",
            return_value={mutmut_workspace.MARKER: selector},
        ):
            child = subprocess_environment()
        self.assertEqual(controlled[mutmut_workspace.MARKER], selector)
        self.assertEqual(child[mutmut_workspace.MARKER], selector)

    def test_controlled_environment_rejects_policy_marker_injection(self) -> None:
        with self.assertRaisesRegex(ValueError, "inject Mutmut policy markers"):
            selector_preserving_environment({mutmut_workspace.MARKER: "stats"})


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""Guard against hidden test outcomes and incomplete cross-platform QA wiring."""

from __future__ import annotations

import ast
import tomllib
import unittest
from pathlib import Path
from typing import Final

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
EXPECTED_CONDITIONAL_TESTS: Final[frozenset[str]] = frozenset()


def _decorator_name(call: ast.Call) -> str | None:
    """Return a recognized conditional-test decorator name.

    Returns:
        The decorator name, or ``None`` for an unrelated call.

    """
    function = call.func
    if not isinstance(function, (ast.Attribute, ast.Name)):
        return None
    name = function.attr if isinstance(function, ast.Attribute) else function.id
    return (
        name
        if name
        in {
            "SkipTest",
            "expectedFailure",
            "importorskip",
            "skip",
            "skipIf",
            "skipUnless",
            "xfail",
        }
        else None
    )


def _reason(call: ast.Call) -> str:
    """Return the conditional-test reason expression in stable source form.

    Returns:
        A literal reason or its explicit constant name.

    """
    for keyword in call.keywords:
        if keyword.arg == "reason":
            if isinstance(keyword.value, ast.Constant) and isinstance(
                keyword.value.value, str
            ):
                return keyword.value.value
            return ast.unparse(keyword.value)
    if not call.args:
        return "<no-reason>"
    argument = call.args[-1]
    if isinstance(argument, ast.Constant) and isinstance(argument.value, str):
        return argument.value
    return ast.unparse(argument)


def _conditional_tests() -> frozenset[str]:
    """Inventory every skip or xfail decorator in the test tree.

    Returns:
        Exact file/decorator/reason contracts.

    """
    conditions: set[str] = set()
    for path in sorted((PROJECT_ROOT / "tests").glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            conditions.update(_node_conditions(path, node))
            conditions.update(_call_conditions(path, node))
            conditions.update(_assignment_conditions(path, node))
    return frozenset(conditions)


def _node_conditions(path: Path, node: ast.AST) -> set[str]:
    """Return recognized conditions declared on one class or function.

    Returns:
        Exact file/decorator/reason contracts for the node.

    """
    if not isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
        return set()
    relative = path.relative_to(PROJECT_ROOT).as_posix()
    conditions: set[str] = set()
    for decorator in node.decorator_list:
        if isinstance(decorator, ast.Call):
            name = _decorator_name(decorator)
            if name is not None:
                conditions.add(f"{relative}:{name}:{_reason(decorator)}")
    return conditions


def _call_conditions(path: Path, node: ast.AST) -> set[str]:
    """Return runtime skip/xfail calls outside decorator positions.

    Returns:
        Exact file/call/reason contracts for the node.

    """
    if not isinstance(node, ast.Call):
        return set()
    name = _decorator_name(node)
    if name is None or name in {"skipIf", "skipUnless", "expectedFailure"}:
        return set()
    relative = path.relative_to(PROJECT_ROOT).as_posix()
    return {f"{relative}:{name}:{_reason(node)}"}


def _assignment_conditions(path: Path, node: ast.AST) -> set[str]:
    """Return module-level pytestmark assignments that can hide outcomes.

    Returns:
        Exact file/assignment/value contracts for the node.

    """
    if not isinstance(node, (ast.Assign, ast.AnnAssign)):
        return set()
    targets = node.targets if isinstance(node, ast.Assign) else [node.target]
    if not any(
        isinstance(target, ast.Name) and target.id == "pytestmark" for target in targets
    ):
        return set()
    relative = path.relative_to(PROJECT_ROOT).as_posix()
    value = "<no-value>" if node.value is None else ast.unparse(node.value)
    return {f"{relative}:pytestmark:{value}"}


class QaInfrastructureContractTests(unittest.TestCase):
    """Require explicit test outcomes, matrices, profiles, and coverage scope."""

    def test_no_hidden_skip_or_xfail_is_introduced(self) -> None:
        self.assertEqual(_conditional_tests(), EXPECTED_CONDITIONAL_TESTS)

    def test_inventory_catches_runtime_and_module_level_bypasses(self) -> None:
        path = PROJECT_ROOT / "tests" / "synthetic.py"
        tree = ast.parse(
            "pytest.skip('runtime')\n"
            "pytest.importorskip('private_dependency')\n"
            "raise unittest.SkipTest('unittest runtime')\n"
            "skip('bare import runtime')\n"
            "pytestmark = pytest.mark.xfail(reason='module')\n"
            "@pytest.mark.xfail(reason='decorator')\n"
            "def test_public(): pass\n"
        )
        conditions: set[str] = set()
        for node in ast.walk(tree):
            conditions.update(_node_conditions(path, node))
            conditions.update(_call_conditions(path, node))
            conditions.update(_assignment_conditions(path, node))
        self.assertTrue(
            {
                "tests/synthetic.py:skip:runtime",
                "tests/synthetic.py:importorskip:private_dependency",
                "tests/synthetic.py:SkipTest:unittest runtime",
                "tests/synthetic.py:skip:bare import runtime",
                "tests/synthetic.py:pytestmark:pytest.mark.xfail(reason='module')",
                "tests/synthetic.py:xfail:decorator",
            }.issubset(conditions)
        )

    def test_both_ci_workflows_have_the_exact_six_way_matrix(self) -> None:
        expected = {
            (system, version)
            for system in ("ubuntu-latest", "macos-latest", "windows-latest")
            for version in ("3.14.7", "3.14.7t")
        }
        for name in ("quality.yml", "hypothesis.yml"):
            content = (PROJECT_ROOT / ".github" / "workflows" / name).read_text(
                encoding="utf-8"
            )
            actual = {
                (system, version)
                for system in ("ubuntu-latest", "macos-latest", "windows-latest")
                for version in ("3.14.7", "3.14.7t")
                if f"- os: {system}\n            python: {version}" in content
            }
            with self.subTest(workflow=name):
                self.assertEqual(actual, expected)
                self.assertIn("Require the POSIX integration environment", content)

    def test_hypothesis_artifacts_publish_only_sanitized_observations(self) -> None:
        content = (PROJECT_ROOT / ".github" / "workflows" / "hypothesis.yml").read_text(
            encoding="utf-8"
        )
        upload_step = content.rpartition(
            "      - name: Publish sanitized Hypothesis observations"
        )[2]
        self.assertIn("path: .hypothesis/observed/", upload_step)
        self.assertNotIn(".hypothesis/examples", content)
        self.assertNotIn("hypothesis-replay-", content)
        self.assertNotIn("actions/cache/", content)
        self.assertNotIn("hypothesis-public", content)
        self.assertEqual(content.count('PYTHONDONTWRITEBYTECODE: "1"'), 1)

    def test_pytest_and_coverage_have_no_failure_hiding_escape_hatches(self) -> None:
        configuration = tomllib.loads(
            (PROJECT_ROOT / "pyproject.toml").read_text(encoding="utf-8")
        )
        pytest_config = configuration["tool"]["pytest"]["ini_options"]
        self.assertEqual(pytest_config["filterwarnings"], ["error"])
        self.assertEqual(pytest_config["required_plugins"], ["hypothesis==6.165.10"])
        self.assertTrue(pytest_config["xfail_strict"])
        self.assertTrue(
            {"--runxfail", "--strict-config", "--strict-markers"}.issubset(
                pytest_config["addopts"]
            )
        )
        coverage = configuration["tool"]["coverage"]
        self.assertEqual(
            coverage["run"]["source"], ["src/eml_attachment_remover", "tools"]
        )
        self.assertTrue(coverage["run"]["branch"])
        self.assertEqual(coverage["report"], {"fail_under": 100, "show_missing": True})
        mutation = configuration["tool"]["mutmut"]
        self.assertEqual(mutation["source_paths"], coverage["run"]["source"])
        self.assertTrue(set(mutation["source_paths"]).isdisjoint(mutation["also_copy"]))
        self.assertEqual(
            configuration["tool"]["hatch"]["build"]["exclude"],
            ["**/*.py.meta", "**/*.py.spans"],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)

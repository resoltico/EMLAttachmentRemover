"""Reject Python modules that accumulate too many responsibilities."""

from __future__ import annotations

import ast
import io
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Iterable

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
SCANNED_DIRECTORIES: Final = ("src", "tests", "tools")
MAX_MODULE_LINES: Final = 450
MAX_SUBSTANTIVE_LINES: Final = 400
MAX_TOP_LEVEL_DECLARATIONS: Final = 20
MAX_CLASS_METHODS: Final = 20
DECLARATION_TYPES: Final = (ast.AsyncFunctionDef, ast.ClassDef, ast.FunctionDef)
METHOD_TYPES: Final = (ast.AsyncFunctionDef, ast.FunctionDef)
UTF8: Final = "utf-8"


@dataclass(frozen=True, slots=True)
class DesignViolation:
    """Describe one structural module-design limit violation."""

    path: Path
    message: str


def _python_files() -> Iterable[Path]:
    """Yield every Python file covered by the repository design budget.

    Yields:
        Source, test, and tool files in deterministic path order.

    """
    for directory in SCANNED_DIRECTORIES:
        yield from sorted((PROJECT_ROOT / directory).rglob("*.py"))


def _line_count(source: str) -> int:
    """Return the physical line count for non-empty source text.

    Returns:
        Zero for an empty file, otherwise its physical line count.

    """
    return len(source.splitlines())


def _substantive_line_count(source: str) -> int:
    """Count nonblank lines that are not comment-only lines.

    Returns:
        The physical lines containing source or literal content.

    """
    lines = source.splitlines()
    comment_only_lines = {
        token.start[0]
        for token in tokenize.generate_tokens(io.StringIO(source).readline)
        if token.type == tokenize.COMMENT
        and not lines[token.start[0] - 1][: token.start[1]].strip()
    }
    return sum(
        bool(line.strip()) and line_number not in comment_only_lines
        for line_number, line in enumerate(lines, start=1)
    )


def _class_method_count(node: ast.ClassDef) -> int:
    """Return the number of direct methods declared by one class.

    Returns:
        The count of direct synchronous and asynchronous methods.

    """
    return sum(isinstance(child, METHOD_TYPES) for child in node.body)


def _module_violations(path: Path) -> list[DesignViolation]:
    """Check one module against complementary anti-god-file budgets.

    Returns:
        Every line, declaration-count, and class-method violation for the module.

    """
    source = path.read_text(encoding=UTF8)
    tree = ast.parse(source, filename=path)
    violations: list[DesignViolation] = []
    line_count = _line_count(source)
    if line_count > MAX_MODULE_LINES:
        violations.append(
            DesignViolation(
                path,
                f"has {line_count} lines; limit is {MAX_MODULE_LINES}",
            ),
        )
    substantive_line_count = _substantive_line_count(source)
    if substantive_line_count > MAX_SUBSTANTIVE_LINES:
        violations.append(
            DesignViolation(
                path,
                f"has {substantive_line_count} substantive lines; limit is "
                f"{MAX_SUBSTANTIVE_LINES}",
            ),
        )
    declarations = [node for node in tree.body if isinstance(node, DECLARATION_TYPES)]
    if len(declarations) > MAX_TOP_LEVEL_DECLARATIONS:
        violations.append(
            DesignViolation(
                path,
                "has "
                f"{len(declarations)} top-level declarations; limit is "
                f"{MAX_TOP_LEVEL_DECLARATIONS}",
            ),
        )
    for node in declarations:
        if isinstance(node, ast.ClassDef):
            _append_class_violation(violations, path, node)
    return violations


def _append_class_violation(
    violations: list[DesignViolation],
    path: Path,
    node: ast.ClassDef,
) -> None:
    """Add an over-large class-method violation when one applies."""
    method_count = _class_method_count(node)
    if method_count > MAX_CLASS_METHODS:
        violations.append(
            DesignViolation(
                path,
                f"class {node.name} has {method_count} methods; "
                f"limit is {MAX_CLASS_METHODS}",
            ),
        )


def main() -> int:
    """Check every tracked Python module and report design violations.

    Returns:
        Zero when all modules meet every limit, otherwise one.

    """
    violations = [
        violation for path in _python_files() for violation in _module_violations(path)
    ]
    for violation in violations:
        relative_path = violation.path.relative_to(PROJECT_ROOT)
        print(f"{relative_path}: {violation.message}", file=sys.stderr)
    return int(bool(violations))


if __name__ == "__main__":
    raise SystemExit(main())

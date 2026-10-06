"""Require every source suppression to have a narrow central approval."""

from __future__ import annotations

import ast
import io
import json
import re
import sys
import tokenize
from pathlib import Path
from typing import Final

ROOT: Final = Path(__file__).resolve().parents[1]
REGISTRY: Final = ROOT / "lint-exceptions.json"
DIRECTIVE: Final = re.compile(r"#\s*(ruff: ignore|type: ignore|noqa)\[([^]]+)\]")
SWIFT_DIRECTIVE: Final = re.compile(r"swiftlint:(?:disable|enable)|swift-format-ignore")


def _owner(tree: ast.AST, line: int) -> str:
    """Find the smallest declaration containing a source directive.

    Returns:
        Its stable name or the module scope.

    """
    declarations = [
        node
        for node in ast.walk(tree)
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef))
        and node.lineno <= line <= (node.end_lineno or node.lineno)
    ]
    return (
        ".".join(
            node.name for node in sorted(declarations, key=lambda node: node.lineno)
        )
        or "<module>"
    )


def _source_anchor(tree: ast.AST, lines: list[str], line: int, column: int) -> str:
    """Bind closing-line or standalone directives to their actual statement.

    Returns:
        Existing meaningful line text, or the canonical owning statement/header.

    Raises:
        ValueError: If a suppression has no identifiable source statement.

    """
    anchor = lines[line - 1][:column].strip()
    if anchor not in {"", ")", "]", "}", "):", "]:", "}:"}:
        return anchor
    statements = [node for node in ast.walk(tree) if isinstance(node, ast.stmt)]
    if not anchor:
        following = [node for node in statements if node.lineno > line]
        selected = min(following, key=lambda node: node.lineno, default=None)
    else:
        owning = [
            node
            for node in statements
            if node.lineno <= line <= (node.end_lineno or node.lineno)
        ]
        selected = min(
            owning,
            key=lambda node: (node.end_lineno or node.lineno) - node.lineno,
            default=None,
        )
    if selected is None:
        message = "Suppression has no identifiable source statement"
        raise ValueError(message)
    text = ast.unparse(selected)
    if isinstance(selected, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        return next(
            value
            for value in text.splitlines()
            if value.startswith(("def ", "async def ", "class "))
        )
    return text


def python_directives(path: Path) -> list[dict[str, str]]:
    """Identify real comment directives, never lookalikes in strings.

    Returns:
        Exact file, declaration, code anchor and directive scopes.

    Raises:
        ValueError: If a blanket suppression is present.

    """
    path = path.resolve()
    source = path.read_text(encoding="utf-8")
    lines = source.splitlines()
    tree = ast.parse(source)
    entries = []
    for token in tokenize.generate_tokens(io.StringIO(source).readline):
        if token.type != tokenize.COMMENT:
            continue
        if re.search(r"#\s*ruff:\s*file-ignore", token.string):
            message = "File-wide Ruff policies belong in pyproject.toml"
            raise ValueError(message)
        match = DIRECTIVE.search(token.string)
        if not match and re.search(
            r"#\s*(ruff:\s*(ignore|noqa)|type: ignore|noqa)\b", token.string
        ):
            message = "Blanket suppression is forbidden"
            raise ValueError(message)
        if match:
            line = token.start[0]
            entries.append({
                "path": path.relative_to(ROOT).as_posix(),
                "scope": _owner(tree, line),
                "anchor": _source_anchor(tree, lines, line, token.start[1]),
                "tool": "mypy" if match[1] == "type: ignore" else "ruff",
                "rules": ",".join(sorted(rule.strip() for rule in match[2].split(","))),
            })
    return entries


def _key(entry: dict[str, str]) -> tuple[str, ...]:
    """Return the exact approved suppression identity.

    Returns:
        Stable scope components excluding its rationale.

    """
    return tuple(entry[name] for name in ("path", "scope", "anchor", "tool", "rules"))


def check() -> list[str]:
    """Check registry coverage and reject hidden Swift source suppressions.

    Returns:
        Every missing, stale or unsupported approval diagnostic.

    """
    data = json.loads(REGISTRY.read_text(encoding="utf-8"))
    approved = data["exceptions"]
    errors = []
    keys = [_key(entry) for entry in approved]
    if len(set(keys)) != len(keys) or any(
        not entry.get("reason", "").strip() for entry in approved
    ):
        errors.append("Exception approvals must be unique and have a rationale")
    observed = {
        _key(entry)
        for directory in ("src", "tests", "tools")
        for path in (ROOT / directory).rglob("*.py")
        for entry in python_directives(path)
    }
    expected = set(keys)
    errors.extend(
        f"Unregistered source suppression: {key}" for key in sorted(observed - expected)
    )
    errors.extend(
        f"Stale source suppression approval: {key}"
        for key in sorted(expected - observed)
    )
    errors.extend(
        f"Swift inline suppression is forbidden: {path.relative_to(ROOT)}"
        for path in (ROOT / "integrations/macos-ui").rglob("*.swift")
        if SWIFT_DIRECTIVE.search(path.read_text(encoding="utf-8"))
    )
    return errors


def main() -> int:
    """Validate the central registry without modifying its approvals.

    Returns:
        One for invalid approvals, otherwise zero.

    """
    errors = check()
    for error in errors:
        print(error, file=sys.stderr)
    return int(bool(errors))


def swift_findings(findings: list[dict[str, object]]) -> list[str]:
    """Apply explicit SwiftLint waivers and fail stale or ambiguous approvals.

    Returns:
        Unapproved diagnostics and invalid exception records.

    """
    approved = json.loads(REGISTRY.read_text(encoding="utf-8"))["swift_exceptions"]
    remaining = {
        (entry["path"], entry["anchor"], entry["rule"]): entry["reason"]
        for entry in approved
    }
    errors = []
    if len(remaining) != len(approved) or any(
        not reason.strip() for reason in remaining.values()
    ):
        errors.append("Swift approvals must be unique and have a rationale")
    for finding in findings:
        path = Path(str(finding["file"]))
        line = int(str(finding["line"]))
        key = (
            path.relative_to(ROOT).as_posix(),
            path.read_text(encoding="utf-8").splitlines()[line - 1].strip(),
            str(finding["rule_id"]),
        )
        if key in remaining:
            del remaining[key]
        else:
            errors.append(
                f"{path.relative_to(ROOT)}:{line}: {finding['rule_id']}: "
                f"{finding['reason']}"
            )
    errors.extend(f"Stale Swift approval: {key}" for key in sorted(remaining))
    return errors


if __name__ == "__main__":
    raise SystemExit(main())

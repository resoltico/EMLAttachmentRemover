"""Narrow registry approvals cannot silently expand or survive deleted code."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest
from tools import lint_exceptions as lint
from tools import mutmut_workspace


def _workspace(root: Path) -> tuple[Path, Path]:
    for name in ("src", "tests", "tools", "integrations/macos-ui"):
        (root / name).mkdir(parents=True)
    source = root / "src/example.py"
    source.write_text(
        'def boundary():\n    value = "# ruff: ignore[not-real]"\n'
        "    return value  # ruff: ignore[private-member-access]\n"
    )
    registry = root / "lint-exceptions.json"
    registry.write_text(json.dumps({"exceptions": [], "swift_exceptions": []}))
    return source, registry


def test_token_scopes_and_registry_exact_approval(tmp_path: Path) -> None:
    source, registry = _workspace(tmp_path)
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        entries = lint.python_directives(source)
        assert len(entries) == 1
        assert entries == [
            {
                "path": "src/example.py",
                "scope": "boundary",
                "anchor": "return value",
                "tool": "ruff",
                "rules": "private-member-access",
            }
        ]
        assert "Unregistered" in lint.check()[0]
        entries[0]["reason"] = "White-box contract test"
        registry.write_text(json.dumps({"exceptions": entries, "swift_exceptions": []}))
        assert lint.check() == []
        source.write_text("pass\n")
        assert "Stale" in lint.check()[0]


@pytest.mark.parametrize("reason", ["", "Approved scoped test"])
def test_duplicate_and_reasonless_approvals_fail(tmp_path: Path, reason: str) -> None:
    source, registry = _workspace(tmp_path)
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        entry = {**lint.python_directives(source)[0], "reason": reason}
        registry.write_text(
            json.dumps({"exceptions": [entry, entry], "swift_exceptions": []})
        )
        assert (
            lint.check()[0] == "Exception approvals must be unique and have a rationale"
        )


@pytest.mark.parametrize("directive", ["# noqa", "# type: ignore", "# ruff: ignore"])
def test_blanket_python_suppression_is_rejected(tmp_path: Path, directive: str) -> None:
    source = tmp_path / "example.py"
    source.write_text("value = 1 " + directive + "\n")
    with (
        patch.object(lint, "ROOT", tmp_path),
        pytest.raises(ValueError, match=r"^Blanket suppression is forbidden$"),
    ):
        lint.python_directives(source)


def test_swift_inline_suppression_is_rejected(tmp_path: Path) -> None:
    _, registry = _workspace(tmp_path)
    (tmp_path / "integrations/macos-ui/Example.swift").write_text(
        "// swiftlint:disable force_try\n"
    )
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        assert "Swift inline suppression" in lint.check()[-1]


@pytest.mark.parametrize("approved", [False, True])
def test_swift_diagnostics_require_exact_central_approval(
    tmp_path: Path, *, approved: bool
) -> None:
    _, registry = _workspace(tmp_path)
    source = tmp_path / "integrations/macos-ui/Example.swift"
    source.write_text("let value = try! task()\n")
    finding: dict[str, object] = {
        "file": str(source),
        "line": 1,
        "rule_id": "force_try",
        "reason": "Unsafe unwrap",
    }
    entry = {
        "path": "integrations/macos-ui/Example.swift",
        "anchor": "let value = try! task()",
        "rule": "force_try",
        "reason": "Negative fixture intentionally traps",
    }
    registry.write_text(
        json.dumps({"exceptions": [], "swift_exceptions": [entry] if approved else []})
    )
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        errors = lint.swift_findings([finding])
        assert errors == (
            []
            if approved
            else [
                f"{Path('integrations/macos-ui/Example.swift')}:1: force_try: Unsafe unwrap"
            ]
        )
        if approved:
            assert "Stale Swift approval" in lint.swift_findings([])[0]
            registry.write_text(
                json.dumps({"exceptions": [], "swift_exceptions": [entry, entry]})
            )
            assert (
                lint.swift_findings([finding])[0]
                == "Swift approvals must be unique and have a rationale"
            )


def test_registry_cli_reports_failures_and_source_registry_passes(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with patch.object(lint, "check", return_value=["Missing approval"]):
        assert lint.main() == 1
    assert "Missing approval" in capsys.readouterr().err
    root = Path(__file__).resolve().parents[1]
    # Registry approvals address original source, not generated mutation wrappers.
    original = root.parent if mutmut_workspace.active(root) else root
    result = subprocess.run(
        [sys.executable, "-B", str(original / "tools/lint_exceptions.py")],
        check=True,
        capture_output=True,
        timeout=30,
    )
    assert not result.stderr


@pytest.mark.parametrize("directory", ["src", "tests", "tools"])
def test_nested_declaration_and_tool_scopes_are_exact(
    tmp_path: Path, directory: str
) -> None:
    _, registry = _workspace(tmp_path)
    source = tmp_path / directory / "nested.py"
    source.write_text(
        "# type: ignore[assignment]\n"
        "class Box:  # ruff: ignore[private-member-access]\n"
        "    async def operation(self):\n"
        "        def inner():  # ruff: ignore[ z-rule , a-rule ]\n"
        "            return 1  # type: ignore[return-value]\n"
    )
    scopes = [
        ("<module>", "", "mypy", "assignment"),
        ("Box", "class Box:", "ruff", "private-member-access"),
        ("Box.operation.inner", "def inner():", "ruff", "a-rule,z-rule"),
        ("Box.operation.inner", "return 1", "mypy", "return-value"),
    ]
    expected = [
        {
            "path": f"{directory}/nested.py",
            "scope": scope,
            "anchor": anchor,
            "tool": tool,
            "rules": rules,
        }
        for scope, anchor, tool, rules in scopes
    ]
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        assert lint.python_directives(source) == expected
        assert any(f"{directory}/nested.py" in error for error in lint.check())
        registry.write_text(
            json.dumps({"exceptions": [expected[0]], "swift_exceptions": []})
        )
        assert (
            lint.check()[0] == "Exception approvals must be unique and have a rationale"
        )


def test_swift_approval_addresses_the_reported_physical_line(tmp_path: Path) -> None:
    _, registry = _workspace(tmp_path)
    source = tmp_path / "integrations/macos-ui/Example.swift"
    source.write_text("let first = 1\n    let second = try! task()\nlet third = 3\n")
    entry = {
        "path": "integrations/macos-ui/Example.swift",
        "anchor": "let second = try! task()",
        "rule": "force_try",
        "reason": "Intentional negative fixture",
    }
    finding = {
        "file": str(source),
        "line": 2,
        "rule_id": "force_try",
        "reason": "Unsafe unwrap",
    }
    registry.write_text(json.dumps({"exceptions": [], "swift_exceptions": [entry]}))
    with patch.object(lint, "ROOT", tmp_path), patch.object(lint, "REGISTRY", registry):
        assert lint.swift_findings([finding]) == []

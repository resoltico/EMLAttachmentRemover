"""Narrow registry approvals cannot silently expand or survive deleted code."""

from __future__ import annotations

import json
import subprocess
import sys
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest
from tools import lint_exceptions as lint

if TYPE_CHECKING:
    from pathlib import Path


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
        assert entries[0]["scope"] == "boundary"
        assert entries[0]["anchor"] == "return value"
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
        assert "unique and have a rationale" in lint.check()[0]


@pytest.mark.parametrize("directive", ["# noqa", "# type: ignore", "# ruff: ignore"])
def test_blanket_python_suppression_is_rejected(tmp_path: Path, directive: str) -> None:
    source = tmp_path / "example.py"
    source.write_text("value = 1 " + directive + "\n")
    with (
        patch.object(lint, "ROOT", tmp_path),
        pytest.raises(ValueError, match="Blanket"),
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
        assert bool(errors) is not approved
        if approved:
            assert "Stale Swift approval" in lint.swift_findings([])[0]
            registry.write_text(
                json.dumps({"exceptions": [], "swift_exceptions": [entry, entry]})
            )
            assert "unique" in lint.swift_findings([finding])[0]


def test_registry_cli_reports_failures_and_source_registry_passes(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with patch.object(lint, "check", return_value=["Missing approval"]):
        assert lint.main() == 1
    assert "Missing approval" in capsys.readouterr().err
    result = subprocess.run(
        [sys.executable, "-B", "tools/lint_exceptions.py"],
        check=True,
        capture_output=True,
        timeout=30,
    )
    assert not result.stderr

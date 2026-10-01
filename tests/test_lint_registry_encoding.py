"""Approval files retain their UTF-8 contract under non-UTF-8 host defaults."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

SCRIPT = r"""
import sys
from pathlib import Path
from tools import lint_exceptions as lint
lint.ROOT = Path(sys.argv[1])
lint.REGISTRY = lint.ROOT / 'lint-exceptions.json'
entries = lint.python_directives(lint.ROOT / 'src/example.py')
assert entries[0]['scope'] == 'caf\u00e9'
assert entries[0]['anchor'] == 'return "\u03c0"'
assert lint.check() == []
finding = {'file': str(lint.ROOT / 'integrations/macos-ui/Example.swift'),
           'line': 1, 'rule_id': 'force_try', 'reason': 'Unsafe unwrap'}
assert lint.swift_findings([finding]) == []
"""


def test_registry_and_sources_decode_utf8_with_ascii_or_windows_locale(
    tmp_path: Path,
) -> None:
    for name in ("src", "tests", "tools", "integrations/macos-ui"):
        (tmp_path / name).mkdir(parents=True)
    (tmp_path / "src/example.py").write_text(
        'def café():\n    return "π"  # ruff: ignore[private-member-access]\n',
        encoding="utf-8",
    )
    (tmp_path / "integrations/macos-ui/Example.swift").write_text(
        "let π = try! task()\n", encoding="utf-8"
    )
    approvals = {
        "exceptions": [
            {
                "path": "src/example.py",
                "scope": "café",
                "anchor": 'return "π"',
                "tool": "ruff",
                "rules": "private-member-access",
                "reason": "Scoped fixture π",
            }
        ],
        "swift_exceptions": [
            {
                "path": "integrations/macos-ui/Example.swift",
                "anchor": "let π = try! task()",
                "rule": "force_try",
                "reason": "Intentional negative fixture π",
            }
        ],
    }
    (tmp_path / "lint-exceptions.json").write_text(
        json.dumps(approvals, ensure_ascii=False), encoding="utf-8"
    )
    environment = dict(os.environ)
    environment.update(LC_ALL="C", PYTHONUTF8="0", PYTHONCOERCECLOCALE="0")
    result = subprocess.run(
        [sys.executable, "-B", "-c", SCRIPT, str(tmp_path)],
        env=environment,
        capture_output=True,
        check=False,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr

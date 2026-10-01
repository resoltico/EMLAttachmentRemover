"""Finder retains validated evidence while treating output finalization as failure."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest

from tests.test_v3_macos_shortcuts_runner import RUNNER, _complete, _item

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX Finder launcher")
@pytest.mark.parametrize("report_kind", ["complete", "noisy", "malformed", "truncated"])
def test_exit_120_retains_only_valid_results_and_never_reveals(
    tmp_path: Path, report_kind: str
) -> None:
    document = _complete(0)
    if report_kind == "noisy":
        document["items"] = [
            _item(
                0,
                "created",
                warnings=[{"code": "WARN", "message": "public warning"}] * 30,
            )
        ]
    if report_kind == "malformed":
        document["summary"] = {}
    encoded = json.dumps(document)
    if report_kind == "truncated":
        encoded = encoded[:20]
    revealed = tmp_path / "revealed"
    opener = tmp_path / "open"
    opener.write_text(f"#!/bin/sh\ntouch '{revealed}'\n", encoding="utf-8")
    opener.chmod(0o700)
    processor = tmp_path / "processor.pyz"
    processor.write_text(
        "import os\nprint(os.environ['FINDER_REPORT'])\nraise SystemExit(120)\n",
        encoding="utf-8",
    )
    result = subprocess.run(
        ["/bin/sh", str(RUNNER), str(tmp_path / "public.eml")],
        env={
            **os.environ,
            "PATH": str(tmp_path) + os.pathsep + os.environ["PATH"],
            "EML_REMOVER_PYTHON": sys.executable,
            "EML_REMOVER_ZIPAPP": str(processor),
            "EML_REMOVER_REVEAL": "1",
            "FINDER_REPORT": encoded,
            "TMPDIR": str(tmp_path),
        },
        capture_output=True,
        text=True,
        check=False,
        timeout=20,
    )
    if report_kind in {"complete", "noisy"}:
        assert result.returncode == 120
        assert "created 1" in result.stdout
        assert "Output finalization failed: processor status 120" in result.stdout
        assert "Interrupted:" not in result.stdout
        if report_kind == "noisy":
            assert "additional diagnostic(s) were omitted" in result.stdout
            assert result.stdout.index(
                "Output finalization failed"
            ) < result.stdout.index("public warning")
    else:
        assert result.returncode == 70
        assert "invalid processor report" in result.stdout
        assert "created 1" not in result.stdout
    assert not revealed.exists()
    assert not list(tmp_path.glob("eml-remover-report.*"))
    assert not list(tmp_path.glob("eml-remover-errors.*"))

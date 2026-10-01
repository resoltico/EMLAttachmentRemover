"""Ordinary subprocess tests import the same application source as their parent."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import eml_attachment_remover


def test_child_imports_the_same_application_source(tmp_path: Path) -> None:
    child = subprocess.run(
        [
            sys.executable,
            "-B",
            "-c",
            "import eml_attachment_remover; print(eml_attachment_remover.__file__)",
        ],
        cwd=tmp_path,
        env=os.environ.copy(),
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )
    assert child.returncode == 0, child.stderr
    assert (
        Path(child.stdout.strip()).resolve()
        == Path(eml_attachment_remover.__file__).resolve()
    )

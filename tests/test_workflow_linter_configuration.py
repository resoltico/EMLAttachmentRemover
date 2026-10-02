"""Check the workflow linter's runner policy without Git discovery."""

from __future__ import annotations

import subprocess
from pathlib import Path


def test_explicit_runner_configuration_works_outside_git(tmp_path: Path) -> None:
    config = Path(__file__).resolve().parents[1] / ".github/actionlint.yaml"
    workflow = tmp_path / "workflow.yml"
    content = (
        "name: Runner check\non: workflow_dispatch\njobs:\n"
        "  check:\n    runs-on: xcode-27\n    steps:\n      - run: true\n"
    )
    workflow.write_text(content)
    command = ["actionlint", "-config-file", str(config), str(workflow)]
    subprocess.run(command, cwd=tmp_path, check=True, capture_output=True, timeout=30)
    workflow.write_text(content.replace("xcode-27", "xcode-27-unknown"))
    rejected = subprocess.run(
        command, cwd=tmp_path, check=False, capture_output=True, text=True, timeout=30
    )
    assert rejected.returncode != 0
    assert 'label "xcode-27-unknown" is unknown' in rejected.stdout + rejected.stderr

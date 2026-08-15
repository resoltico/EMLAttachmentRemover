"""Require bytecode-free Python execution in every GitHub Actions job."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
WORKFLOW_DIRECTORY: Final = PROJECT_ROOT / ".github" / "workflows"
EXPECTED_PYTHON_JOBS: Final = {
    "hypothesis.yml": frozenset({"explore"}),
    "mutation.yml": frozenset({"mutation"}),
    "quality.yml": frozenset({"quality"}),
    "release.yml": frozenset({"build", "mutation", "publish", "qualify"}),
}
EXPECTED_LOCKED_SYNCS: Final = {
    "hypothesis.yml": 1,
    "mutation.yml": 1,
    "quality.yml": 1,
    "release.yml": 3,
}
JOB_HEADER: Final = re.compile(r"^  (?P<name>[A-Za-z0-9_-]+):$")
PYTHON_USE: Final = re.compile(
    r"(?:\bCPython\b|\bpython(?:-version|3(?:\.\d+)*)?\b|\buv run\b)",
    re.IGNORECASE,
)
BYTECODE_SETTING: Final = '      PYTHONDONTWRITEBYTECODE: "1"'
LOCKED_SYNC: Final = re.compile(
    r"      - name: Install locked development dependencies\n"
    r"        run: uv sync --locked --group dev --python [^\n]+\n"
    r"        env:\n"
    r"          PYTHONWARNINGS: default(?:\n|$)",
)


def _workflow_job_blocks(path: Path) -> dict[str, str]:
    """Return the top-level job bodies from one workflow.

    Returns:
        A mapping from job identifier to its YAML body.

    """
    jobs_started = False
    current_job: str | None = None
    blocks: dict[str, list[str]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line == "jobs:":
            jobs_started = True
            continue
        if not jobs_started:
            continue
        if line and not line.startswith(" "):
            break
        match = JOB_HEADER.fullmatch(line)
        if match is not None:
            current_job = match.group("name")
            blocks[current_job] = []
        elif current_job is not None:
            blocks[current_job].append(line)
    return {name: "\n".join(lines) for name, lines in blocks.items()}


def _python_jobs(blocks: dict[str, str]) -> frozenset[str]:
    """Return the jobs whose definitions install or invoke Python.

    Returns:
        The identifiers of Python-running jobs.

    """
    return frozenset(name for name, body in blocks.items() if PYTHON_USE.search(body))


def test_python_job_inventory_is_exact() -> None:
    """Make additions or removals of Python-running jobs an explicit change."""
    for workflow_name, expected_jobs in EXPECTED_PYTHON_JOBS.items():
        blocks = _workflow_job_blocks(WORKFLOW_DIRECTORY / workflow_name)
        assert _python_jobs(blocks) == expected_jobs


def test_every_python_job_disables_bytecode_at_job_scope() -> None:
    """Prevent any Python-running job from creating repository bytecode."""
    for workflow_name in EXPECTED_PYTHON_JOBS:
        blocks = _workflow_job_blocks(WORKFLOW_DIRECTORY / workflow_name)
        for job_name in _python_jobs(blocks):
            setting_count = blocks[job_name].splitlines().count(BYTECODE_SETTING)
            assert setting_count == 1, f"{workflow_name}:{job_name}"


def test_dependency_installation_does_not_promote_third_party_warnings() -> None:
    """Keep warnings strict for project checks without breaking dependency builds."""
    for workflow_name, expected_count in EXPECTED_LOCKED_SYNCS.items():
        workflow = (WORKFLOW_DIRECTORY / workflow_name).read_text(encoding="utf-8")
        assert workflow.count("- name: Install locked development dependencies") == (
            expected_count
        )
        assert len(LOCKED_SYNC.findall(workflow)) == expected_count

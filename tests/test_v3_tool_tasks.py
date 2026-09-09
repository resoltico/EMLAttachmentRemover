"""Task-runner contracts for the v3 quality, mutation, and release lanes."""

from __future__ import annotations

from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
TASKS = PROJECT_ROOT / "tools" / "tasks.py"
MUTATION_RESULTS = PROJECT_ROOT / "tools" / "check_mutation_results.py"
MUTATION_TASK = PROJECT_ROOT / "tools" / "mutation_task.py"
WORKFLOWS = PROJECT_ROOT / ".github" / "workflows"


def test_quality_task_keeps_static_hygiene_and_branch_coverage_gates() -> None:
    task_source = TASKS.read_text(encoding="utf-8")
    assert "tools/check_repository_hygiene.py" in task_source
    assert '"ruff", "format", "--check"' in task_source
    assert '"mypy", "--no-incremental"' in task_source
    assert "tools/check_module_design.py" in task_source
    assert "tools/report_coverage.py" in task_source
    assert "detect-secrets-hook" in task_source


def test_mutation_task_binds_results_to_both_core_and_assurance_sources() -> None:
    task_source = TASKS.read_text(encoding="utf-8")
    result_source = MUTATION_RESULTS.read_text(encoding="utf-8")
    assert 'PROJECT_ROOT / "src" / "eml_attachment_remover"' in result_source
    assert 'PROJECT_ROOT / "tools"' in result_source
    assert "equivalent_mutants.json" in task_source
    assert "check_mutation_results.py" in MUTATION_TASK.read_text(encoding="utf-8")


def test_workflow_lanes_invoke_the_matching_quality_mutation_and_release_tasks() -> (
    None
):
    quality = (WORKFLOWS / "quality.yml").read_text(encoding="utf-8")
    mutation = (WORKFLOWS / "mutation.yml").read_text(encoding="utf-8")
    release = (WORKFLOWS / "release.yml").read_text(encoding="utf-8")
    assert "tools/tasks.py quality" in quality
    assert "tools/tasks.py mutation" in mutation
    assert "tools/tasks.py quality" in release
    assert "tools/tasks.py mutation" in release
    assert "tools/tasks.py release" in release

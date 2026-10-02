"""Workflow contracts for superseded runs and re-run-safe evidence artifacts."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final

WORKFLOWS: Final = Path(__file__).resolve().parents[1] / ".github" / "workflows"
ARTIFACT_NAME: Final = re.compile(r"^ +name: (.+)$", re.MULTILINE)


def _text(name: str) -> str:
    return (WORKFLOWS / name).read_text(encoding="utf-8")


def test_only_pull_request_exploration_is_cancelled_when_superseded() -> None:
    hypothesis = _text("hypothesis.yml")
    assert (
        "  group: hypothesis-${{ github.workflow }}-${{ github.ref }}\n"
        "  # A newer PR revision supersedes exploration of the old one; scheduled, "
        "push,\n"
        "  # and manual campaigns are deliberate and still queue.\n"
        "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n"
    ) in hypothesis
    for name in ("mutation.yml", "release.yml"):
        assert "  cancel-in-progress: false\n" in _text(name)


def test_diagnostic_artifact_names_are_unique_per_run_attempt() -> None:
    names = [
        name
        for path in sorted(WORKFLOWS.glob("*.yml"))
        for name in ARTIFACT_NAME.findall(path.read_text(encoding="utf-8"))
        if "github.run_id" in name
    ]
    assert len(names) == 7
    assert len(set(names)) == len(names)
    assert all(
        name.endswith("-${{ github.run_id }}-${{ github.run_attempt }}")
        for name in names
    )


def test_release_qualification_keeps_its_quality_reports() -> None:
    release = _text("release.yml")
    qualify = release[release.index("  qualify:") : release.index("  property:")]
    assert "if: always()" in qualify
    assert (
        "name: release-quality-${{ runner.os }}-${{ matrix.python }}-"
        "${{ github.run_id }}-${{ github.run_attempt }}"
    ) in qualify
    assert (
        "build/test-results-project-ci.xml\n            build/coverage.xml" in qualify
    )
    build = release[release.index("  build:") : release.index("  publish:")]
    assert "      - fuzz\n" in build
    assert "    uses: ./.github/workflows/swift-fuzz.yml\n" in release
    assert "  workflow_call:\n" in _text("swift-fuzz.yml")


def test_mutation_workers_are_benchmarkable_but_default_to_the_host_policy() -> None:
    assert "      MUTATION_WORKERS: ${{ inputs.workers || 'auto' }}\n" in _text(
        "mutation.yml"
    )
    assert "      MUTATION_WORKERS: auto\n" in _text("release.yml")

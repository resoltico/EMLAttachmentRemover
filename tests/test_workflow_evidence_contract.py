"""Workflow contracts for superseded runs and re-run-safe evidence artifacts."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final

WORKFLOWS: Final = Path(__file__).resolve().parents[1] / ".github" / "workflows"
ARTIFACT_NAME: Final = re.compile(r"^          name: (.+)$", re.MULTILINE)


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
    steps = [
        step
        for path in sorted(WORKFLOWS.glob("*.yml"))
        for step in re.split(r"(?m)^      - ", path.read_text(encoding="utf-8"))
        if "uses: actions/upload-artifact@" in step
    ]
    names_per_step = [ARTIFACT_NAME.findall(step) for step in steps]
    assert all(len(found) == 1 for found in names_per_step)
    names = [found[0] for found in names_per_step]
    assert names
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


def test_publication_waits_for_the_tagged_archives_on_supported_macos() -> None:
    release = _text("release.yml")
    compatibility = release[
        release.index("  compatibility:") : release.index("  publish:")
    ]
    publish = release[release.index("  publish:") :]
    assert "    needs: build\n" in compatibility
    assert "os: [macos-14, macos-15-intel, xcode-27]" in compatibility
    assert (
        "name: release-dist-${{ github.run_id }}-${{ github.run_attempt }}"
        in compatibility
    )
    assert "-m tools.release_delivery --verify-directory release-dist" in compatibility
    assert "EML_DELIVERY_DIRECTORY: release-dist" in compatibility
    assert "      - compatibility\n" in publish


def test_mutation_workers_are_benchmarkable_but_default_to_the_host_policy() -> None:
    assert "      MUTATION_WORKERS: ${{ inputs.workers || 'auto' }}\n" in _text(
        "mutation.yml"
    )
    assert "      MUTATION_WORKERS: auto\n" in _text("release.yml")


def test_release_and_compatibility_enforce_uv_managed_interpreters() -> None:
    """CI must not silently switch to newly available system Python builds."""
    for name in ("macos-compatibility.yml", "release.yml"):
        assert "env:\n  UV_PYTHON_PREFERENCE: only-managed\n" in _text(name)


def test_mutation_requalifies_main_when_assurance_inputs_change() -> None:
    """Do not leave a source-changing merge without fresh mutation evidence."""
    workflow = _text("mutation.yml")
    assert "  push:\n    branches: [main]\n    paths:\n" in workflow
    for path in (
        ".github/workflows/mutation.yml",
        "src/**",
        "tools/**",
        "tests/**",
        ".python-version",
        "pyproject.toml",
        "uv.lock",
    ):
        assert f'      - "{path}"\n' in workflow
    assert "  schedule:\n" in workflow
    assert "  workflow_dispatch:\n" in workflow
    assert "  cancel-in-progress: false\n" in workflow


def test_mac14_consumer_remains_a_true_os14_gate_after_runner_cutover() -> None:
    """Runner substitution cannot quietly replace minimum-OS execution."""
    selection = (
        "matrix.os == 'macos-14' && "
        "fromJSON(vars.EML_MACOS14_RUNS_ON || '\"macos-14\"') || matrix.os"
    )
    action = (
        Path(__file__).resolve().parents[1]
        / ".github/actions/sonoma-compatibility/action.yml"
    ).read_text(encoding="utf-8")
    for name in ("macos-compatibility.yml", "release.yml"):
        content = _text(name)
        assert selection in content
        assert "os: [macos-14, macos-15-intel, xcode-27]" in content
        assert "uses: ./.github/actions/sonoma-compatibility" in content
        assert "sudo xcode-select --switch" not in content
    for check in ("sw_vers -productVersion", "uname -m", "arch -x86_64", "Xcode 16.2"):
        assert check in action

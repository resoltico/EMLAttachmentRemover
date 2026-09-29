"""The mutation diagnostics bundle is complete where it can be and bounded always."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Final
from unittest.mock import patch

import pytest
from tools import mutation_diagnostics, mutation_task, tasks

RESULTS: Final = (
    "pkg.mod.x_a__mutmut_1: killed\n"
    "pkg.mod.x_a__mutmut_2: survived\n"
    "pkg.mod.x_b__mutmut_1: no tests\n"
    "pkg.mod.x_b__mutmut_2: timeout\n"
    "pkg.mod.x_c__mutmut_1: killed\n"
)


def _write(
    tmp_path: Path, results: str = RESULTS, stats: object = None
) -> tuple[Path, Path]:
    results_path = tmp_path / "results.txt"
    results_path.write_text(results, encoding="utf-8")
    stats_path = tmp_path / "stats.json"
    if stats is not None:
        stats_path.write_text(json.dumps(stats), encoding="utf-8")
    return results_path, stats_path


def _entry(output: Path) -> dict[str, object]:
    index = json.loads((output / "index.json").read_text(encoding="utf-8"))
    return dict(index["mutants"][0])


def _show(mutant: str) -> str:
    return f"--- a\n+++ b\n@@ {mutant} @@\n"


def test_every_unkilled_mutant_gets_a_patch_its_status_and_mapped_tests(
    tmp_path: Path,
) -> None:
    """Killed mutants are omitted; each other one is fully described."""
    results, stats = _write(
        tmp_path,
        stats={
            "tests_by_mangled_function_name": {
                "pkg.mod.x_a": ["tests/test_a.py::one", "tests/test_a.py::two"]
            }
        },
    )
    output = tmp_path / "bundle"
    mutation_diagnostics.collect(results, stats, output, _show)
    index = json.loads((output / "index.json").read_text(encoding="utf-8"))
    assert index["not_killed"] == 3
    assert index["listed"] == 3
    assert index["by_status"] == {"no tests": 1, "survived": 1, "timeout": 1}
    first, second, third = index["mutants"]
    assert first == {
        "mutant": "pkg.mod.x_a__mutmut_2",
        "status": "survived",
        "patch": "0001.diff",
        "patch_truncated": False,
        "tests": ["tests/test_a.py::one", "tests/test_a.py::two"],
        "tests_truncated": False,
    }
    assert (second["tests"], third["patch"]) == ([], "0003.diff")
    assert (output / "0001.diff").read_text(encoding="utf-8") == _show(
        "pkg.mod.x_a__mutmut_2"
    )
    assert sorted(path.name for path in output.iterdir()) == [
        "0001.diff",
        "0002.diff",
        "0003.diff",
        "index.json",
    ]


def test_the_bundle_is_bounded_and_says_what_it_left_out(tmp_path: Path) -> None:
    """Counts, patch bytes, and test lists are all capped, with truncation recorded."""
    total = mutation_diagnostics.MAX_MUTANTS + 5
    results, stats = _write(
        tmp_path,
        "".join(f"pkg.x_f__mutmut_{n}: survived\n" for n in range(1, total + 1)),
        {
            "tests_by_mangled_function_name": {
                "pkg.x_f": [f"t{n}" for n in range(mutation_diagnostics.MAX_TESTS + 3)]
            }
        },
    )
    output = tmp_path / "bundle"
    big = "x" * (mutation_diagnostics.MAX_DIFF_BYTES + 10)
    mutation_diagnostics.collect(results, stats, output, lambda _mutant: big)
    index = json.loads((output / "index.json").read_text(encoding="utf-8"))
    assert (index["not_killed"], index["listed"]) == (
        total,
        mutation_diagnostics.MAX_MUTANTS,
    )
    entry = index["mutants"][0]
    assert entry["patch_truncated"] is True
    assert entry["tests_truncated"] is True
    assert len(entry["tests"]) == mutation_diagnostics.MAX_TESTS
    assert (output / "0001.diff").stat().st_size == mutation_diagnostics.MAX_DIFF_BYTES
    assert len(list(output.iterdir())) == mutation_diagnostics.MAX_MUTANTS + 1


def test_an_exactly_full_patch_and_test_list_are_not_reported_truncated(
    tmp_path: Path,
) -> None:
    """The bounds are inclusive."""
    results, stats = _write(
        tmp_path,
        "pkg.x_f__mutmut_1: survived\n",
        {
            "tests_by_mangled_function_name": {
                "pkg.x_f": ["t"] * mutation_diagnostics.MAX_TESTS
            }
        },
    )
    output = tmp_path / "bundle"
    exact = "y" * mutation_diagnostics.MAX_DIFF_BYTES
    mutation_diagnostics.collect(results, stats, output, lambda _mutant: exact)
    entry = json.loads((output / "index.json").read_text(encoding="utf-8"))["mutants"][
        0
    ]
    assert (entry["patch_truncated"], entry["tests_truncated"]) == (False, False)


@pytest.mark.parametrize(
    "failure",
    [subprocess.CalledProcessError(1, "mutmut"), OSError("gone")],
)
def test_an_unavailable_patch_is_recorded_not_fatal(
    tmp_path: Path, failure: Exception
) -> None:
    """One mutant that cannot be shown must not cost the rest of the evidence."""
    results, stats = _write(tmp_path, "pkg.x_f__mutmut_1: survived\n")
    output = tmp_path / "bundle"

    def show(_mutant: str) -> str:
        raise failure

    mutation_diagnostics.collect(results, stats, output, show)
    entry = json.loads((output / "index.json").read_text(encoding="utf-8"))["mutants"][
        0
    ]
    assert entry["patch"] == "patch unavailable"
    assert not (output / "0001.diff").exists()


def test_a_previous_bundle_is_replaced_and_an_all_killed_run_lists_nothing(
    tmp_path: Path,
) -> None:
    """Stale patches never outlive the campaign that wrote them."""
    output = tmp_path / "bundle"
    output.mkdir()
    (output / "0009.diff").write_text("stale", encoding="utf-8")
    results, stats = _write(tmp_path, "pkg.x_f__mutmut_1: killed\n")
    mutation_diagnostics.collect(results, stats, output, _show)
    assert [path.name for path in output.iterdir()] == ["index.json"]
    index = json.loads((output / "index.json").read_text(encoding="utf-8"))
    assert (index["not_killed"], index["listed"], index["mutants"]) == (0, 0, [])


def test_show_mutant_asks_mutmut_for_exactly_that_mutant() -> None:
    """The patch is fetched with a finite timeout, decoded, from the project root."""
    completed = subprocess.CompletedProcess(("m",), 0, stdout="diff text")
    paths = mutation_task.mutation_paths(
        Path("root"), Path("root/build"), Path("s"), Path("r"), Path("e")
    )
    with patch("tools.mutation_task.subprocess.run", return_value=completed) as run:
        text = mutation_task.show_mutant(
            paths, "python", {"K": "v"}, "pkg.x_a__mutmut_2"
        )
    assert text == "diff text"
    run.assert_called_once_with(
        ("python", "-m", "mutmut", "show", "pkg.x_a__mutmut_2"),
        check=True,
        cwd=Path("root"),
        env={"K": "v"},
        timeout=mutation_task.EVIDENCE_TIMEOUT_SECONDS,
        capture_output=True,
        encoding="utf-8",
    )


def test_the_task_collects_diagnostics_from_the_campaign_evidence() -> None:
    """The task wires the canonical evidence paths and the mutant patch runner."""
    calls: list[tuple[Path, Path, Path, str]] = []

    def collect(results: Path, stats: Path, output: Path, show: object) -> None:
        calls.append((results, stats, output, show("m")))  # type: ignore[operator]

    with (
        patch.object(tasks.mutation_diagnostics, "collect", collect),
        patch.object(tasks.mutation_task, "show_mutant", lambda *args: args[3]),
    ):
        tasks._capture_mutation_diagnostics()  # ruff: ignore[private-member-access] - task contract.
    assert calls == [
        (
            tasks.MUTATION_RESULTS,
            tasks.PROJECT_ROOT / "mutants" / "mutmut-stats.json",
            tasks.BUILD_DIRECTORY / "mutation-diagnostics",
            "m",
        )
    ]


def test_a_campaign_without_captured_results_bundles_nothing(tmp_path: Path) -> None:
    """Its own failure is reported elsewhere; no bundle is invented."""
    output = tmp_path / "bundle"
    mutation_diagnostics.collect(
        tmp_path / "missing.txt", tmp_path / "stats.json", output, _show
    )
    assert not output.exists()

"""Behavioral contracts for per-family generated-property evidence."""

from __future__ import annotations

import json
import runpy
from pathlib import Path

import pytest
from tools import check_v301_property_observations as gate


def _manifest(path: Path, families: object) -> Path:
    path.write_text(
        json.dumps({"schema_version": 1, "families": families}), encoding="utf-8"
    )
    return path


def _observations(directory: Path, records: list[object]) -> Path:
    directory.mkdir()
    (directory / "run_testcases.jsonl").write_text(
        "".join(json.dumps(record) + "\n" for record in records), encoding="utf-8"
    )
    return directory


def _record(property_id: object, *, passed: bool = True) -> dict[str, object]:
    return {
        "property": property_id,
        "status": "passed" if passed else "failed",
        "type": "test_case",
    }


def test_gate_accepts_one_genuine_passed_record_per_declared_family(
    tmp_path: Path,
) -> None:
    manifest = _manifest(tmp_path / "families.json", {"wire": ["wire-id"]})
    observed = _observations(tmp_path / "observed", [_record("wire-id")])
    assert gate._families(manifest) == {"wire": ("wire-id",)}  # ruff: ignore[private-member-access] - manifest receipt.
    assert gate._observed_properties(observed) == {"wire-id"}  # ruff: ignore[private-member-access] - record receipt.
    gate.check(manifest, observed)


@pytest.mark.parametrize(
    ("content", "message"),
    [
        ("", "cannot load v3 property-family manifest"),
        ("[]", gate.MANIFEST_ERROR),
        ('{"schema_version": 0, "families": {}}', gate.MANIFEST_ERROR),
        ('{"schema_version": 1, "families": []}', gate.MANIFEST_ERROR),
    ],
)
def test_manifest_loader_rejects_absent_or_invalid_public_shapes(
    tmp_path: Path, content: str, message: str
) -> None:
    manifest = tmp_path / "families.json"
    if content:
        manifest.write_text(content, encoding="utf-8")
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate._families(manifest)  # ruff: ignore[private-member-access] - strict manifest parsing.
    assert str(rejected.value) == message


@pytest.mark.parametrize(
    "families",
    [
        {"one": "one"},
        {"one": []},
        {"one": [""]},
        ["not-a-family-mapping"],
    ],
)
def test_manifest_loader_rejects_invalid_family_entries(
    tmp_path: Path, families: object
) -> None:
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate._families(_manifest(tmp_path / "families.json", families))  # ruff: ignore[private-member-access] - strict family entry grammar.
    assert str(rejected.value) == gate.MANIFEST_ERROR


@pytest.mark.parametrize(
    "records",
    [
        ["not an object"],
        [{"type": "test_case", "status": "passed", "property": ""}],
    ],
)
def test_observation_loader_rejects_invalid_generated_case_records(
    tmp_path: Path, records: list[object]
) -> None:
    observed = _observations(tmp_path / "observed", records)
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate._observed_properties(observed)  # ruff: ignore[private-member-access] - strict generated record grammar.
    assert str(rejected.value) == gate.OBSERVATION_ERROR


def test_observation_loader_rejects_invalid_json(tmp_path: Path) -> None:
    observed = tmp_path / "observed"
    observed.mkdir()
    (observed / "run_testcases.jsonl").write_text("{\n", encoding="utf-8")
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate._observed_properties(observed)  # ruff: ignore[private-member-access] - strict JSONL parsing.
    assert str(rejected.value) == gate.OBSERVATION_ERROR


def test_observation_loader_ignores_nonpassed_or_noncase_records(
    tmp_path: Path,
) -> None:
    observed = _observations(
        tmp_path / "observed",
        [_record("failed", passed=False), {"type": "info", "property": "info"}],
    )
    assert gate._observed_properties(observed) == set()  # ruff: ignore[private-member-access] - only passed generated cases qualify.


def test_gate_reports_each_family_without_a_generated_case(tmp_path: Path) -> None:
    manifest = _manifest(
        tmp_path / "families.json", {"wire": ["wire-id"], "policy": ["policy-id"]}
    )
    observed = _observations(tmp_path / "observed", [_record("wire-id")])
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate.check(manifest, observed)
    assert str(rejected.value) == "v3 property families lack observations: policy"


def test_gate_joins_multiple_missing_families_with_the_public_separator(
    tmp_path: Path,
) -> None:
    """Every missing property family is visible in one stable diagnostic."""
    manifest = _manifest(
        tmp_path / "families.json",
        {"wire": ["wire-id"], "policy": ["policy-id"], "ledger": ["ledger-id"]},
    )
    observed = _observations(tmp_path / "observed", [_record("wire-id")])
    with pytest.raises(gate.PropertyObservationError) as rejected:
        gate.check(manifest, observed)
    assert (
        str(rejected.value) == "v3 property families lack observations: policy, ledger"
    )


def test_main_reports_success_and_failure_from_configured_paths(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    manifest = _manifest(tmp_path / "families.json", {"wire": ["wire-id"]})
    observed = _observations(tmp_path / "observed", [_record("wire-id")])
    monkeypatch.setattr(gate, "MANIFEST", manifest)
    monkeypatch.setattr(gate, "OBSERVED", observed)
    assert gate.main() == 0
    assert (
        capsys.readouterr().out
        == "v3 property observations cover every declared family\n"
    )
    (observed / "run_testcases.jsonl").unlink()
    assert gate.main() == 1
    assert capsys.readouterr().out == "v3 property observations are absent\n"


def test_gate_uses_literal_utf8_for_every_public_evidence_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Public manifests and JSONL observations never inherit a locale encoding."""
    manifest = _manifest(tmp_path / "families.json", {"wire": ["wire-id"]})
    observed = _observations(tmp_path / "observed", [_record("wire-id")])
    source = observed / "run_testcases.jsonl"
    calls: list[str | None] = []
    original = Path.read_text

    def read_text(
        path: Path, encoding: str | None = None, errors: str | None = None
    ) -> str:
        calls.append(encoding)
        return original(path, encoding=encoding, errors=errors)

    monkeypatch.setattr(Path, "read_text", read_text)
    assert gate._families(manifest) == {"wire": ("wire-id",)}  # ruff: ignore[private-member-access] - manifest encoding contract.
    assert gate._properties_in_file(source) == {"wire-id"}  # ruff: ignore[private-member-access] - observation encoding contract.
    assert calls == ["utf-8", "utf-8"]


def test_script_entrypoint_executes_the_documented_main() -> None:
    """The standalone qualification command always terminates through ``main``."""
    with pytest.raises(SystemExit):
        runpy.run_path(gate.__file__, run_name="__main__")

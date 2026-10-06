"""Finder launcher rendering contracts for schema-3 processor reports."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Final, cast

import pytest

RUNNER: Final = (
    Path(__file__).resolve().parents[1]
    / "integrations"
    / "macos-ui"
    / "processing-launcher.sh"
)
STATUSES: Final = (
    "created",
    "existing_verified",
    "would_create",
    "failed",
    "cancelled",
    "not_run",
    "published_with_error",
)


def test_every_private_interpreter_invocation_is_isolated_and_cache_free() -> None:
    """All launcher helpers preserve sealed runtimes by disabling bytecode writes."""
    commands = [
        line.strip()
        for line in RUNNER.read_text(encoding="utf-8").splitlines()
        if line.strip().startswith('"$PYTHON" ')
    ]
    assert len(commands) == 5
    assert all(command.startswith('"$PYTHON" -I -B ') for command in commands)


def _path(
    display: str, text: str | None = None, native: str | None = None
) -> dict[str, str | None]:
    return {
        "text": text,
        "display": display,
        "native_base64": native,
        "native_utf16le_base64": None,
    }


def _item(
    index: int,
    status: str,
    *,
    error: dict[str, object] | None = None,
    warnings: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    publication: dict[str, object] | None = None
    if status in {"created", "existing_verified"}:
        publication = {
            "visibility": "visible" if status == "created" else "existing_verified",
            "address_verified": True,
            "final_address": _path("accepted", f"/private/accepted-{index}.eml"),
        }
    return {
        "index": index,
        "phase": "published",
        "status": status,
        "terminalized": True,
        "source_request": _path(f"source-{index}"),
        "destination_request": None,
        "source": None,
        "destination": None,
        "transformation": None,
        "verification": None,
        "publication": publication,
        "warnings": [] if warnings is None else warnings,
        "error": error,
    }


def _report(items: list[dict[str, object]], status: int) -> dict[str, object]:
    counts = dict.fromkeys(STATUSES, 0)
    for item in items:
        counts[str(item["status"])] += 1
    counts["total"] = len(items)
    return {
        "schema_version": 3,
        "scope": "mime-pruned",
        "program": "remove-eml-attachments",
        "version": "3.0.6",
        "mode": "apply",
        "ok": False,
        "exit_code": status,
        "interrupted": True,
        "interruption": {
            "signal": "SIGINT",
            "reason": "interrupted",
            "phase": "report",
        },
        "batch_error": {"code": "BATCH_FAILURE", "message": "batch summary"},
        "summary": counts,
        "items": items,
    }


def _run(
    tmp_path: Path, report: dict[str, object], status: int
) -> subprocess.CompletedProcess[str]:
    zipapp = tmp_path / "processor.pyz"
    zipapp.write_text(
        "import json, os, sys\n"
        "print(os.environ['FINDER_REPORT'])\n"
        "raise SystemExit(int(os.environ['FINDER_STATUS']))\n",
        encoding="utf-8",
    )
    source = tmp_path / "source.eml"
    source.write_bytes(b"source")
    environment = {
        **os.environ,
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_ZIPAPP": str(zipapp),
        "FINDER_REPORT": json.dumps(report),
        "FINDER_STATUS": str(status),
    }
    shell = "/bin/sh" if os.name != "nt" else "sh"
    return subprocess.run(
        [shell, str(RUNNER), str(source)],
        check=False,
        capture_output=True,
        cwd=tmp_path,
        encoding="utf-8",
        env=environment,
    )


def test_finder_launcher_visibly_projects_every_terminal_category(
    tmp_path: Path,
) -> None:
    """A mixed result displays counts, item diagnostics, and batch interruption."""
    report = _report(
        [
            _item(0, "created", warnings=[{"code": "WARN", "message": "retained"}]),
            _item(1, "failed", error={"code": "PARSE_ERROR", "message": "broken"}),
            _item(2, "not_run", error={"code": "BATCH_FAILURE", "message": "later"}),
            _item(
                3,
                "published_with_error",
                error={
                    "code": "PUBLICATION_INCOMPLETE",
                    "message": "visible but unproven",
                },
            ),
            _item(4, "cancelled", error={"code": "INTERRUPTED", "message": "stopped"}),
        ],
        9,
    )
    result = _run(tmp_path, report, 9)
    assert result.returncode == 9
    assert not result.stderr
    envelope = json.loads(result.stdout)
    assert envelope["report"] == report
    assert envelope["process_status"] == 9
    assert envelope["details"] == [
        "Batch: BATCH_FAILURE: batch summary",
        "Interrupted: interrupted",
    ]


def test_missing_selected_runtime_never_uses_available_discovery(
    tmp_path: Path,
) -> None:
    """An explicit runtime selection fails instead of silently changing interpreters."""
    fallback = tmp_path / "bin"
    fallback.mkdir()
    interpreter = fallback / "python3.14"
    interpreter.write_text('#!/bin/sh\nprintf reached > "$FALLBACK_MARKER"\nexit 0\n')
    interpreter.chmod(0o755)
    marker = tmp_path / "fallback-used"
    processor = tmp_path / "processor.pyz"
    processor.write_text("raise SystemExit(0)\n")
    result = subprocess.run(
        ["/bin/sh" if os.name != "nt" else "sh", str(RUNNER), "public.eml"],
        env={
            **os.environ,
            "PATH": str(fallback) + os.pathsep + os.environ["PATH"],
            "EML_REMOVER_PYTHON": str(tmp_path / "missing-python"),
            "EML_REMOVER_ZIPAPP": str(processor),
            "FALLBACK_MARKER": str(marker),
        },
        capture_output=True,
        check=False,
        timeout=15,
    )
    assert result.returncode == 9
    assert b"selected CPython 3.14 runtime is missing" in result.stderr
    assert not marker.exists()
    assert json.loads(result.stdout) == {
        "launcher_error": "python_unavailable",
        "process_status": 9,
    }


def test_finder_launcher_fails_closed_for_summary_exit_and_schema_drift(
    tmp_path: Path,
) -> None:
    """A malformed canonical report never becomes a Finder success message."""
    report = _report([_item(0, "created")], 9)
    summary = cast("dict[str, int]", report["summary"])
    report["summary"] = {**summary, "created": 0}
    result = _run(tmp_path, report, 9)
    assert result.returncode == 70
    assert not result.stderr
    assert result.stdout.startswith("EML Attachment Remover: invalid processor report:")
    assert "Processor diagnostics were withheld" in result.stdout


def test_finder_launcher_rejects_ok_true_when_a_batch_error_exists(
    tmp_path: Path,
) -> None:
    """A batch-wide failure makes the producer's success claim invalid."""
    report = _report([_item(0, "created")], 7)
    report["ok"] = True
    report["interrupted"] = False
    report["interruption"] = None
    result = _run(tmp_path, report, 7)
    assert result.returncode == 70
    assert "report ok value does not match item receipts" in result.stdout


@pytest.mark.skipif(os.name == "nt", reason="POSIX native-byte contract")
def test_finder_launcher_accepts_native_only_posix_output_address(
    tmp_path: Path,
) -> None:
    """A native-only proven POSIX address is not confused with its display text."""
    report = _report([_item(0, "created")], 0)
    report["ok"] = True
    report["batch_error"] = None
    report["interrupted"] = False
    report["interruption"] = None
    items = cast("list[dict[str, object]]", report["items"])
    item = items[0]
    publication = cast("dict[str, object]", item["publication"])
    publication["final_address"] = _path("native-only", None, "bmF0aXZlLf8uZW1s")
    result = _run(tmp_path, report, 0)
    assert result.returncode == 0
    assert json.loads(result.stdout)["report"]["summary"]["created"] == 1


def _complete(status: int) -> dict[str, object]:
    """Build a complete, uninterrupted, successful report with a given exit field.

    Returns:
        A structurally valid report of one created output.

    """
    report = _report([_item(0, "created")], status)
    report["ok"] = True
    report["batch_error"] = None
    report["interrupted"] = False
    report["interruption"] = None
    return report


def test_finder_launcher_accepts_a_signal_that_arrived_during_delivery(
    tmp_path: Path,
) -> None:
    """A complete report with status 130 keeps its receipts and says so."""
    result = _run(tmp_path, _complete(0), 130)
    assert result.returncode == 130
    assert json.loads(result.stdout)["report"]["summary"]["created"] == 1
    assert "Interrupted: after the report was written" in result.stdout
    assert "invalid processor report" not in result.stdout


@pytest.mark.parametrize("status", [2, 9, 70])
def test_finder_launcher_rejects_every_other_status_disagreement(
    tmp_path: Path, status: int
) -> None:
    """Only the interruption status may differ from the delivered document."""
    result = _run(tmp_path, _complete(0), status)
    assert result.returncode == 70
    assert "report identity or exit status is invalid" in result.stdout


def test_finder_launcher_still_rejects_a_truncated_report_on_interruption(
    tmp_path: Path,
) -> None:
    """A partially delivered document is never mistaken for a complete one."""
    zipapp = tmp_path / "processor.pyz"
    zipapp.write_text(
        'print(\'{"schema_version": 3, "scope"\')\nraise SystemExit(130)\n',
        encoding="utf-8",
    )
    source = tmp_path / "source.eml"
    source.write_bytes(b"source")
    result = subprocess.run(
        ["/bin/sh" if os.name != "nt" else "sh", str(RUNNER), str(source)],
        check=False,
        capture_output=True,
        cwd=tmp_path,
        encoding="utf-8",
        env={
            **os.environ,
            "EML_REMOVER_PYTHON": sys.executable,
            "EML_REMOVER_ZIPAPP": str(zipapp),
        },
    )
    assert result.returncode == 70
    assert "invalid processor report" in result.stdout


def test_native_ui_transport_preserves_receipts_and_invocation_failure(
    tmp_path: Path,
) -> None:
    """The native UI gets admitted receipts even when final invocation fails."""
    report = _report([_item(0, "created"), _item(1, "published_with_error")], 9)
    result = _run(tmp_path, report, 120)
    assert result.returncode == 120
    envelope = json.loads(result.stdout)
    assert envelope["report"] == report
    assert envelope["process_status"] == 120
    assert envelope["details"][0].startswith("Output finalization failed:")


def test_native_ui_transport_rejects_unadmitted_reports(tmp_path: Path) -> None:
    """Malformed receipt counts cannot reach the UI as machine reports."""
    report = _report([_item(0, "created")], 9)
    report["summary"] = {}
    result = _run(tmp_path, report, 9)
    assert result.returncode == 70
    with pytest.raises(json.JSONDecodeError):
        json.loads(result.stdout)

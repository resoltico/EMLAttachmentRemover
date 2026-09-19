"""Finder launcher rendering contracts for schema-3 processor reports."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Final, cast

RUNNER: Final = (
    Path(__file__).resolve().parents[1]
    / "integrations"
    / "macos-shortcuts"
    / "run-from-finder.sh"
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


def _path(display: str, text: str | None = None) -> dict[str, str | None]:
    return {
        "text": text,
        "display": display,
        "native_base64": None,
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
        "version": "3.0.4",
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
        "EML_REMOVER_REVEAL": "0",
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
    assert (
        "created 1; existing verified 0; failed 1; not run 1; "
        "published with error 1; cancelled 1" in result.stdout
    )
    assert "source-0: WARN: retained" in result.stdout
    assert "source-1: PARSE_ERROR: broken" in result.stdout
    assert "source-2: BATCH_FAILURE: later" in result.stdout
    assert "source-3: PUBLICATION_INCOMPLETE: visible but unproven" in result.stdout
    assert "source-4: INTERRUPTED: stopped" in result.stdout
    assert "Batch: BATCH_FAILURE: batch summary" in result.stdout
    assert "Interrupted: interrupted" in result.stdout


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

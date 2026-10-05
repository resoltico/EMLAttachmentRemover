"""Destination selection reaches the real processor without shell interpretation."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest
from tools.build_zipapp import build_zipapp

from tests.test_processing_launcher import RUNNER


@pytest.mark.parametrize(
    ("kind", "status"), [("default", 0), ("folder", 0), ("missing", 7), ("file", 7)]
)
def test_destination_selection_is_applied_without_fallback(
    tmp_path: Path, kind: str, status: int
) -> None:
    """A selected destination is honored or refused; the original is unchanged."""
    processor = build_zipapp(tmp_path / "processor.pyz", verify=True)
    source = tmp_path / "input -- name.eml"
    raw = b"Content-Type: text/plain; charset=x-opaque\r\n\r\nretained\r\n"
    source.write_bytes(raw)
    destination = tmp_path / "copies -- selected folder"
    if kind == "folder":
        destination.mkdir()
    elif kind == "file":
        destination.write_bytes(b"not a directory")
    environment = dict(os.environ)
    environment.pop("EML_REMOVER_OUTPUT_DIR", None)
    environment.update({
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_ZIPAPP": str(processor),
    })
    if kind != "default":
        environment["EML_REMOVER_OUTPUT_DIR"] = str(destination)
    result = subprocess.run(
        ["/bin/sh" if os.name != "nt" else "sh", str(RUNNER), str(source)],
        env=environment,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == status, result.stderr
    envelope = json.loads(result.stdout)
    assert envelope["process_status"] == status
    item = envelope["report"]["items"][0]
    assert item["warnings"] == []
    assert source.read_bytes() == raw
    beside = source.with_suffix(".mime-pruned.eml")
    selected = destination / Path(item["destination_request"]["text"]).name
    if status == 0:
        output = beside if kind == "default" else selected
        assert output.read_bytes() == raw
        assert item["status"] == "created"
        if kind == "folder":
            assert not beside.exists()
    else:
        assert item["status"] == "failed"
        assert item["error"]["code"] == "WRITE_ERROR"
        assert not beside.exists()

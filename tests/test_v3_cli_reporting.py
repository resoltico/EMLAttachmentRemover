"""Schema-3 CLI rendering and migration contracts."""

from __future__ import annotations

import json
import os
from typing import TYPE_CHECKING

from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.cli import main
from eml_attachment_remover.domain import ItemStatus

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def _options() -> BatchOptions:
    return BatchOptions(
        dry_run=True,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def test_dry_run_honors_explicit_destination_without_creating_it(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "message.eml"
    destination = tmp_path / "chosen.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert (
        main(["--dry-run", "--output-format=json", "-o", str(destination), str(source)])
        == 0
    )
    document = json.loads(capsys.readouterr().out)
    assert document["items"][0]["status"] == "would_create"
    assert document["items"][0]["destination_request"]["text"] == str(destination)
    assert not destination.exists()


def test_dry_run_has_explicit_not_attempted_publication_receipt(
    tmp_path: Path,
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    ledger = execute([str(source)], _options())
    receipt = ledger.items[0].publication
    assert ledger.items[0].status is ItemStatus.WOULD_CREATE
    assert receipt is not None
    assert receipt.visibility == "not_attempted"
    assert receipt.file_sync == "not_attempted"
    assert receipt.final_address is None


def test_v2_switches_and_newline_paths_return_usage_migration(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert main(["--force", str(source)]) == 2
    assert "removed in v3" in capsys.readouterr().err
    assert main(["--output-format=paths", str(source)]) == 2
    assert "paths0" in capsys.readouterr().err


def test_requested_json_usage_failure_has_batch_error_and_source_request(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert main(["--output-format=json", "--force", str(source)]) == 2
    document = json.loads(capsys.readouterr().out)
    assert document["batch_error"]["code"] == "USAGE"
    assert document["items"][0]["source_request"]["text"] == str(source)
    assert document["items"][0]["status"] == "not_run"


def test_paths0_emits_only_exact_accepted_destination_bytes(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    source_name = "odd-unicode-π.eml" if os.name == "nt" else "odd\nname.eml"
    source = tmp_path / source_name
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert main(["--output-format=paths0", str(source)]) == 0
    output, _errors = capfd.readouterr()
    expected = os.fsencode(str(source.with_suffix(".mime-pruned.eml"))) + b"\0"
    assert output.encode() == expected


def test_paths0_keeps_success_warnings_on_standard_error(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "opaque-charset.eml"
    source.write_bytes(
        b"Content-Type: text/plain; charset=does-not-exist\r\n\r\nbody\r\n"
    )
    assert main(["--output-format=paths0", str(source)]) == 0
    _output, errors = capfd.readouterr()
    assert "CHARSET_PRESERVED_OPAQUE" in errors
    assert str(source) in errors


def test_json_schema_three_has_one_terminal_record_per_requested_input(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    good = tmp_path / "good.eml"
    missing = tmp_path / "missing.eml"
    good.write_bytes(b"Content-Type: text/plain\r\n\r\ngood\r\n")
    assert main(["--output-format=json", str(good), str(missing)]) == 9
    document = json.loads(capsys.readouterr().out)
    assert document["schema_version"] == 3
    assert document["scope"] == "mime-pruned"
    assert document["summary"]["total"] == 2
    assert [item["index"] for item in document["items"]] == [0, 1]
    assert [item["status"] for item in document["items"]] == ["created", "failed"]
    assert document["items"][1]["source_request"]["text"] == str(missing)

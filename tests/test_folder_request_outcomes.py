"""Folder batches explain whole-selection failures and retain explicit hidden intent."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch, report_stream, selection_collection
from eml_attachment_remover.domain import AppError, ItemStatus
from tests.test_request_transport import _frame, _payload, _run

if TYPE_CHECKING:
    from pathlib import Path

MESSAGE = b"Subject: Public folder QA\r\n\r\nretained\r\n"


def test_dot_entries_are_skipped_but_explicit_selections_are_retained(
    tmp_path: Path,
) -> None:
    hidden = tmp_path / ".hidden"
    hidden.mkdir()
    visible = tmp_path / "Invoice.eml"
    sidecar = tmp_path / "._Invoice.eml"
    inside = hidden / "h.eml"
    for path in [visible, sidecar, inside]:
        path.write_bytes(MESSAGE)
    assert selection_collection.collect([str(tmp_path)]) == [str(visible)]
    assert selection_collection.collect([str(sidecar), str(hidden)]) == [
        str(sidecar),
        str(inside),
    ]


def test_observed_count_limit_reports_truthful_threshold_and_remedy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    for index in range(4):
        (tmp_path / f"{index}.eml").write_bytes(MESSAGE)
    monkeypatch.setattr(selection_collection, "MAX_BATCH_ITEMS", 3)
    with pytest.raises(AppError) as error:
        selection_collection.collect([str(tmp_path)])
    assert error.value.phase == "selection"
    assert error.value.message == (
        "The selection contains more than 3 email files. "
        "Choose a subfolder or select fewer files. No copies were created."
    )
    assert not list(tmp_path.glob("*.mime-pruned.eml"))


def test_unavailable_chosen_folder_fails_once_before_framed_processing(
    tmp_path: Path,
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(MESSAGE)
    status, output, stderr = _run(
        _frame(_payload([str(source)])),
        arguments=("--output-dir", str(tmp_path / "missing")),
    )
    assert status == 7, stderr
    report = json.loads(output)
    assert report["batch_error"]["phase"] == "destination"
    assert (
        "Reconnect the volume or choose another folder"
        in report["batch_error"]["message"]
    )
    assert not list(tmp_path.glob("*.mime-pruned.eml"))
    assert source.read_bytes() == MESSAGE


def test_same_basename_outputs_are_stable_across_batch_order_and_subsets(
    tmp_path: Path,
) -> None:
    sources = []
    for name in ["one", "two"]:
        parent = tmp_path / name
        parent.mkdir()
        source = parent / "m1.eml"
        source.write_bytes(MESSAGE)
        sources.append(str(source))
    destination = tmp_path / "copies"
    destination.mkdir()
    options = batch.BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=str(destination),
    )
    observed = []
    for selected in [sources, list(reversed(sources)), sources[:1]]:
        ledger = batch.execute(selected, options)
        try:
            assert all(
                item.status in {ItemStatus.CREATED, ItemStatus.EXISTING_VERIFIED}
                for item in ledger.items
            )
            destinations = {}
            for item in ledger.items:
                assert item.destination_request is not None
                destinations[item.source_request.text] = item.destination_request.text
            observed.append(destinations)
        finally:
            report_stream.close(ledger)
    assert observed[0] == observed[1]
    assert observed[2][sources[0]] == observed[0][sources[0]]
    assert len(set(observed[0].values())) == 2
    assert len(list(destination.glob("*.mime-pruned.eml"))) == 2

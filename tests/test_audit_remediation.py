"""Regression contracts for the 2026 audit-remediation requirements."""

from __future__ import annotations

import io
import json
import os
import subprocess
import sys
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import (
    mime_headers,
    mime_raw,
    native_binding,
    native_posix,
    report_stream,
    reporting_v3,
    staged_output,
)
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    BoundDestination,
    ExitCode,
    ItemStatus,
    PathValue,
)
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.native_values import report_path_bytes

if TYPE_CHECKING:
    from pathlib import Path


def test_root_unix_from_envelope_keeps_original_coordinates_and_prunes_attachment() -> (
    None
):
    """Keep one root envelope while removing only a declared attachment."""
    envelope = b"From sender@example.test\r\n"
    root = (
        b"From: a@example.test\r\n"
        b"To: b@example.test\r\n"
        b"MIME-Version: 1.0\r\n"
        b"Content-Type: multipart/mixed; boundary=B\r\n\r\n"
    )
    keep = b"--B\r\nContent-Type: text/plain\r\n\r\nKEEP THIS BODY\r\n"
    remove = (
        b"--B\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment; filename=x.bin\r\n\r\nREMOVE\r\n"
    )
    end = b"--B--\r\n"
    source = envelope + root + keep + remove + end
    tree = parse_raw_mime(source)
    plan = classify(tree.root)
    candidate = build_candidate(tree, plan.removals)
    assert candidate.raw == envelope + root + keep + end
    assert tree.root.start == 0
    assert tree.root.headers[0].start == len(envelope)


@pytest.mark.parametrize("ending", [b"\r\n", b"\n", b"\r"])
def test_root_unix_from_envelope_supports_each_message_line_ending(
    ending: bytes,
) -> None:
    """A single root envelope remains byte-identical for every supported ending."""
    raw = b"From sender@example.test" + ending + b"Subject: retained" + ending + ending
    tree = parse_raw_mime(raw)
    assert tree.root.headers[0].start == len(b"From sender@example.test" + ending)


def test_duplicate_root_unix_from_envelopes_are_rejected() -> None:
    """Two consecutive transport envelopes are ambiguous rather than a body default."""
    raw = b"From one\nFrom two\nSubject: retained\n\nbody\n"
    with pytest.raises(AppError) as rejected:
        parse_raw_mime(raw)
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "multiple Unix-From envelope lines"
    )


def test_destination_parent_open_failure_is_a_write_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Destination environmental failures retain their output-side classification."""
    monkeypatch.setattr(
        native_posix,
        "_directory",
        lambda _path: (_ for _ in ()).throw(PermissionError("denied")),
    )
    with pytest.raises(AppError) as rejected:
        native_posix._bind_destination("out.eml", "out.eml")  # ruff: ignore[private-member-access] - destination error role.
    assert rejected.value.code is ExitCode.WRITE_ERROR


def test_prepublication_staging_os_error_is_contextual_write_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A real staging-side OS failure is not promoted to an internal failure."""
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - lifecycle boundary regression.
        cast("BoundDestination", object()), b"candidate", "digest"
    )
    monkeypatch.setattr(staged_output, "_bind_parent", lambda _state: None)
    monkeypatch.setattr(
        staged_output,
        "_create_stage",
        lambda _state: (_ for _ in ()).throw(PermissionError("denied")),
    )
    primary, cleanup = staged_output._run_lifecycle(state)  # ruff: ignore[private-member-access] - staging classification.
    assert cleanup == ("succeeded", None)
    assert primary == AppError(
        ExitCode.WRITE_ERROR, "could not stage candidate: denied"
    )


def test_cli_json_transport_is_ascii_under_a_hostile_stdout_codec(
    tmp_path: Path,
) -> None:
    """JSON output bytes do not inherit the process text stdout encoding."""
    source = tmp_path / "Ērvins.eml"
    source.write_bytes(b"From: a@example.test\r\n\r\nretained\r\n")
    environment = {**os.environ, "PYTHONIOENCODING": "utf-16"}
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "eml_attachment_remover",
            "--output-format",
            "json",
            str(source),
        ],
        check=False,
        capture_output=True,
        env=environment,
    )
    assert result.returncode == 0
    assert result.stdout.startswith(b"{")
    assert not result.stdout.startswith((b"\xff\xfe", b"\xfe\xff"))
    report = json.loads(result.stdout.decode("ascii"))
    assert report["ok"] is True
    assert report["items"][0]["status"] == "created"


def test_json_embedding_adapter_uses_the_same_ascii_text_without_buffer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A text-only embedding stream receives the canonical ASCII JSON text."""
    stream = io.StringIO()
    monkeypatch.setattr(sys, "stdout", stream)
    reporting_v3.write_json({"display": "Ērvins"})
    assert stream.getvalue() == '{"display": "\\u0112rvins"}\n'


def test_report_path_bytes_rejects_missing_or_nul_native_evidence() -> None:
    """Accepted-path delivery never invents a native address from absent evidence."""
    with pytest.raises(
        ValueError, match=r"^accepted final address lacks native path evidence$"
    ) as missing:
        report_path_bytes(PathValue(None, "native-only", None))
    assert str(missing.value) == "accepted final address lacks native path evidence"
    with pytest.raises(
        ValueError, match=r"^accepted final address contains NUL$"
    ) as nul:
        report_path_bytes(PathValue("safe", "safe", "YQBi"))
    assert str(nul.value) == "accepted final address contains NUL"
    with pytest.raises(ValueError, match="base64"):
        report_path_bytes(PathValue(None, "native-only", "%%"))


def test_existing_destination_open_error_is_a_contextual_write_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An environment failure is neither a conflict nor an internal error."""
    monkeypatch.setattr(
        native_binding,
        "_open_bound_destination",
        lambda _destination: (_ for _ in ()).throw(PermissionError("denied")),
    )
    with pytest.raises(AppError) as rejected:
        native_binding._existing_identity(  # ruff: ignore[private-member-access] - destination inspection role.
            cast("BoundDestination", object())
        )
    assert rejected.value == AppError(
        ExitCode.WRITE_ERROR, "could not inspect destination: denied"
    )


def test_destination_parent_error_keeps_its_contextual_message(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Destination binding retains an actionable write-side diagnostic."""
    monkeypatch.setattr(
        native_posix,
        "_parent",
        lambda _path: (_ for _ in ()).throw(
            AppError(ExitCode.INPUT_ERROR, "source-style parent failure")
        ),
    )
    with pytest.raises(AppError) as rejected:
        native_posix._bind_destination("out.eml", "out.eml")  # ruff: ignore[private-member-access] - destination message contract.
    assert rejected.value == AppError(
        ExitCode.WRITE_ERROR,
        "could not open destination parent: source-style parent failure",
    )


def test_root_envelope_requires_a_first_header_name_and_first_colon() -> None:
    """Envelope recognition cannot accept a malformed first header token."""
    raw = b"From sender\r\n: malformed\r\n\r\nbody\r\n"
    assert mime_headers.root_header_start(raw, 0, len(raw)) == 0


def test_nonroot_entity_never_receives_root_envelope_treatment() -> None:
    """Nested From bytes remain headerless body data even when a header follows."""
    raw = b"From sender\r\nX-Value: retained\r\n\r\nbody\r\n"
    headers, body_start, header_bytes = mime_raw._entity_headers(  # ruff: ignore[private-member-access] - nonroot envelope boundary.
        raw, 0, len(raw), root=False
    )
    assert (headers, body_start, header_bytes) == ((), 0, 0)
    # ruff: ignore[private-member-access] - default must remain non-root.
    (
        default_headers,
        default_body_start,
        default_header_bytes,
    ) = mime_raw._entity_headers(raw, 0, len(raw))
    assert (default_headers, default_body_start, default_header_bytes) == ((), 0, 0)


def test_root_envelope_keeps_a_colon_bearing_first_header_at_its_first_colon() -> None:
    """A later colon in the first header value cannot change root ownership."""
    raw = b"From sender\r\nX-Value: one: two\r\n\r\nbody\r\n"
    tree = parse_raw_mime(raw)
    assert tree.root.headers[0].value == b"one: two"


def test_report_path_text_is_null_at_both_surrogate_boundaries() -> None:
    """Every surrogate code point remains native-only report evidence."""
    for surrogate in ("\ud800", "\udfff"):
        value = reporting_v3._path(  # ruff: ignore[private-member-access] - schema projection boundary.
            PathValue(f"name-{surrogate}", "safe", "bmFtZQ==")
        )
        assert value is not None
        assert value["text"] is None


def test_postedge_os_error_reconciles_with_the_fixed_failed_receipt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An already-visible candidate preserves its exact reconciliation mode."""
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - postedge lifecycle branch.
        cast("BoundDestination", object()),
        b"candidate",
        "digest",
        kernel_published=True,
    )
    calls: list[str] = []
    monkeypatch.setattr(
        staged_output,
        "_bind_parent",
        lambda _state: (_ for _ in ()).throw(OSError("after edge")),
    )

    def reconcile(_state: object, value: str) -> object:
        calls.append(value)
        return None

    monkeypatch.setattr(
        staged_output,
        "_reconcile",
        reconcile,
    )
    staged_output._run_lifecycle(state)  # ruff: ignore[private-member-access] - reconciliation mode.
    assert calls == ["failed"]


def test_streamed_json_rejects_nonfinite_values_in_each_writer(
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Neither streamed JSON writer may emit a nonstandard nonfinite number."""
    with pytest.raises(ValueError, match="Out of range float"):
        report_stream._write_pair(  # ruff: ignore[private-member-access] - pair JSON policy.
            "value", float("nan"), terminal=True
        )
    ledger = BatchLedger.from_requests([PathValue("source", "source", None)])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    record = reporting_v3.item_json(ledger.items[0])
    bad_record = {**record, "warnings": [{"value": float("nan")}]}
    monkeypatch.setattr(report_stream, "_records", lambda _ledger: iter((bad_record,)))
    with pytest.raises(ValueError, match="Out of range float"):
        report_stream._write_item_array(ledger)  # ruff: ignore[private-member-access] - item JSON policy.
    assert capsys.readouterr().out == '"value": "items": ['


def test_streamed_json_escapes_unicode_in_each_incremental_writer(
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """All incremental report writers preserve the canonical ASCII transport."""
    report_stream._write_pair(  # ruff: ignore[private-member-access] - pair transport contract.
        "value", {"name": "Ērvins"}, terminal=True
    )
    assert capsys.readouterr().out == '"value": {"name": "\\u0112rvins"}'
    ledger = BatchLedger.from_requests([PathValue("source", "source", None)])
    monkeypatch.setattr(
        report_stream,
        "_records",
        lambda _ledger: iter(({"name": "Ērvins"},)),
    )
    report_stream._write_item_array(ledger)  # ruff: ignore[private-member-access] - item transport contract.
    assert capsys.readouterr().out == '"items": [{"name": "\\u0112rvins"}]'


def test_preedge_os_error_never_attempts_postpublication_reconciliation(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The reconciliation path is reserved exclusively for a crossed edge."""
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - pre-edge lifecycle regression.
        cast("BoundDestination", object()), b"candidate", "digest"
    )
    calls: list[str] = []
    monkeypatch.setattr(staged_output, "_bind_parent", lambda _state: None)
    monkeypatch.setattr(
        staged_output,
        "_create_stage",
        lambda _state: (_ for _ in ()).throw(PermissionError("denied")),
    )
    monkeypatch.setattr(
        staged_output,
        "_reconcile",
        lambda _state, value: calls.append(value),
    )
    staged_output._run_lifecycle(state)  # ruff: ignore[private-member-access] - pre-edge reconciliation boundary.
    assert calls == []


def test_overlong_default_destination_is_item_local_and_later_work_continues(
    tmp_path: Path,
) -> None:
    """A filesystem basename limit does not stop unrelated later source work."""
    long_source = tmp_path / ("a" * 240 + ".eml")
    good_source = tmp_path / "good.eml"
    source_bytes = b"From: a@example.test\r\n\r\nretained\r\n"
    long_source.write_bytes(source_bytes)
    good_source.write_bytes(source_bytes)
    options = BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )
    ledger = execute([str(long_source), str(good_source)], options)
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.CREATED,
    ]
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.code is ExitCode.WRITE_ERROR
    assert not (tmp_path / ("a" * 240 + ".mime-pruned.eml")).exists()
    assert (tmp_path / "good.mime-pruned.eml").read_bytes() == source_bytes


def test_overlong_default_destination_respects_fail_fast(tmp_path: Path) -> None:
    """Fail-fast leaves later items not-run after the same local write failure."""
    long_source = tmp_path / ("b" * 240 + ".eml")
    later_source = tmp_path / "later.eml"
    source_bytes = b"From: a@example.test\r\n\r\nretained\r\n"
    long_source.write_bytes(source_bytes)
    later_source.write_bytes(source_bytes)
    options = BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=True,
        output=None,
        output_dir=None,
    )
    ledger = execute([str(long_source), str(later_source)], options)
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert not (tmp_path / "later.mime-pruned.eml").exists()

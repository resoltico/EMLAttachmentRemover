"""Complete schema-3 reporting receipts and public-channel contracts."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

from eml_attachment_remover import reporting_v3
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    BoundDestination,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    Removal,
    RemovalReason,
    RetainedFingerprint,
    SourceSnapshot,
    TransformationPlan,
    VerificationReceipt,
)

if TYPE_CHECKING:
    import pytest


def _complete_ledger() -> BatchLedger:
    source_request = PathValue(
        "source-request",
        "source π display",
        "c291cmNlLXJlcXVlc3Q=",
    )
    destination_request = PathValue(
        "destination-request",
        "destination display",
        "ZGVzdGluYXRpb24tcmVxdWVzdA==",
    )
    source = SourceSnapshot(
        request=source_request,
        expanded=PathValue(
            "source-expanded", "expanded display", "c291cmNlLWV4cGFuZGVk"
        ),
        parent=PathValue(
            "source-parent", "source parent display", "c291cmNlLXBhcmVudA=="
        ),
        basename=b"source.bin",
        final_address=PathValue(
            "source-final", "source final display", "c291cmNlLWZpbmFs"
        ),
        identity=FileIdentity(10, 11, "-regular", 12),
        mode=0o100600,
        raw=b"source bytes",
        digest="source-sha256",
        size=456,
    )
    destination = BoundDestination(
        request=destination_request,
        parent=PathValue(
            "destination-parent",
            "destination parent display",
            "ZGVzdGluYXRpb24tcGFyZW50",
        ),
        basename=b"destination.bin",
        directory_identity=FileIdentity(20, 21, "directory", 22),
    )
    plan = TransformationPlan(
        removals=(
            Removal((2, 1), "application/pdf", RemovalReason.EXPLICIT_ATTACHMENT),
            Removal((3,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
        ),
        retained=(
            RetainedFingerprint(
                (1, 3),
                "text/plain",
                "quoted-printable",
                ((b"first", b"first-value"), (b"second", b"second-value")),
                "encoded-sha256",
                "decoded-sha256",
            ),
        ),
        stripped_headers=("Content-Length", "X-Attachment"),
        candidate_sha256="candidate-sha256",
        candidate_size=789,
        candidate=b"candidate bytes",
    )
    published_address = PathValue(
        "published.bin", "published display", "cHVibGlzaGVkLmJpbg=="
    )
    created = LedgerItem(7, source_request, destination_request)
    created.phase = ItemPhase.PUBLISHED
    created.source = source
    created.destination = destination
    created.transformation = plan
    created.verification = VerificationReceipt(
        output_parses=True,
        retained_payloads_match=False,
        structure_matches=True,
        policy_is_idempotent=False,
        digest_matches=True,
    )
    created.publication = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(40, 41, "regular", 42),
        digest="published-sha256",
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=published_address,
        temp_cleanup="succeeded",
    )
    created.warnings.append({"code": "CREATED_WARN", "message": "created warning"})
    created.finish(ItemStatus.CREATED)

    failed = LedgerItem(
        8,
        PathValue("failed-request", "failed display", "ZmFpbGVkLXJlcXVlc3Q="),
    )
    failed.warnings.append({"code": "FAILED_WARN", "message": "failed warning"})
    failed.finish(
        ItemStatus.FAILED,
        AppError(ExitCode.PARSE_ERROR, "failed message", (4, 5), "parsed"),
    )
    return BatchLedger(
        [created, failed],
        batch_error=AppError(ExitCode.BATCH_FAILURE, "batch message", (9,), "batch"),
    )


def test_report_serializes_every_complete_receipt_and_schema_field() -> None:
    """The JSON document keeps every receipt rather than reporting a summary only."""
    document = reporting_v3.report(_complete_ledger(), "apply", 9)

    def path(text: str, display: str, native: str) -> dict[str, str | None]:
        return {
            "text": text,
            "display": display,
            "native_base64": native,
            "native_utf16le_base64": None,
        }

    assert document == {
        "schema_version": 3,
        "scope": "mime-pruned",
        "program": "remove-eml-attachments",
        "version": "3.0.0",
        "mode": "apply",
        "ok": False,
        "exit_code": 9,
        "interrupted": False,
        "interruption": None,
        "batch_error": {
            "code": "BATCH_FAILURE",
            "message": "batch message",
            "mime_path": (9,),
            "phase": "batch",
        },
        "summary": {
            "created": 1,
            "existing_verified": 0,
            "would_create": 0,
            "failed": 1,
            "cancelled": 0,
            "not_run": 0,
            "published_with_error": 0,
            "total": 2,
        },
        "items": [
            {
                "index": 7,
                "phase": "published",
                "status": "created",
                "terminalized": True,
                "source_request": path(
                    "source-request", "source π display", "c291cmNlLXJlcXVlc3Q="
                ),
                "destination_request": path(
                    "destination-request",
                    "destination display",
                    "ZGVzdGluYXRpb24tcmVxdWVzdA==",
                ),
                "source": {
                    "expanded": path(
                        "source-expanded", "expanded display", "c291cmNlLWV4cGFuZGVk"
                    ),
                    "parent": path(
                        "source-parent",
                        "source parent display",
                        "c291cmNlLXBhcmVudA==",
                    ),
                    "basename_base64": "c291cmNlLmJpbg==",
                    "basename_utf16le_base64": None,
                    "final_address": path(
                        "source-final", "source final display", "c291cmNlLWZpbmFs"
                    ),
                    "identity": {
                        "device": "10",
                        "inode": "11",
                        "type": "-regular",
                        "ctime_ns": "12",
                    },
                    "sha256": "source-sha256",
                    "size": 456,
                },
                "destination": {
                    "parent": path(
                        "destination-parent",
                        "destination parent display",
                        "ZGVzdGluYXRpb24tcGFyZW50",
                    ),
                    "basename_base64": "ZGVzdGluYXRpb24uYmlu",
                    "basename_utf16le_base64": None,
                    "final_address": path(
                        "destination-request",
                        "destination display",
                        "ZGVzdGluYXRpb24tcmVxdWVzdA==",
                    ),
                    "identity": {
                        "device": "20",
                        "inode": "21",
                        "type": "directory",
                        "ctime_ns": "22",
                    },
                },
                "transformation": {
                    "retained": [
                        {
                            "source_mime_path": (1, 3),
                            "content_type": "text/plain",
                            "cte": "quoted-printable",
                            "content_type_parameters": [
                                {
                                    "name_base64": "Zmlyc3Q=",
                                    "value_base64": "Zmlyc3QtdmFsdWU=",
                                },
                                {
                                    "name_base64": "c2Vjb25k",
                                    "value_base64": "c2Vjb25kLXZhbHVl",
                                },
                            ],
                            "encoded_sha256": "encoded-sha256",
                            "decoded_sha256": "decoded-sha256",
                        }
                    ],
                    "removal_roots": [
                        {
                            "mime_path": (2, 1),
                            "content_type": "application/pdf",
                            "reason": "EXPLICIT_ATTACHMENT",
                        },
                        {
                            "mime_path": (3,),
                            "content_type": "image/png",
                            "reason": "RELATED_NONROOT_COMPONENT",
                        },
                    ],
                    "stripped_headers": ["Content-Length", "X-Attachment"],
                    "candidate_sha256": "candidate-sha256",
                    "candidate_size": 789,
                },
                "verification": {
                    "output_parses": True,
                    "retained_payloads_match": False,
                    "structure_matches": True,
                    "policy_is_idempotent": False,
                    "digest_matches": True,
                },
                "publication": {
                    "visibility": "visible",
                    "identity": {
                        "device": "40",
                        "inode": "41",
                        "type": "regular",
                        "ctime_ns": "42",
                    },
                    "sha256": "published-sha256",
                    "file_sync": "succeeded",
                    "directory_sync": "succeeded",
                    "address_verified": True,
                    "final_address": path(
                        "published.bin", "published display", "cHVibGlzaGVkLmJpbg=="
                    ),
                    "temp_cleanup": "succeeded",
                },
                "warnings": [{"code": "CREATED_WARN", "message": "created warning"}],
                "error": None,
            },
            {
                "index": 8,
                "phase": "requested",
                "status": "failed",
                "terminalized": True,
                "source_request": path(
                    "failed-request", "failed display", "ZmFpbGVkLXJlcXVlc3Q="
                ),
                "destination_request": None,
                "source": None,
                "destination": None,
                "transformation": None,
                "verification": None,
                "publication": None,
                "warnings": [{"code": "FAILED_WARN", "message": "failed warning"}],
                "error": {
                    "code": "PARSE_ERROR",
                    "message": "failed message",
                    "mime_path": (4, 5),
                    "phase": "parsed",
                },
            },
        ],
    }


def test_complete_receipt_uses_canonical_json_and_human_channels(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Structured and human channels disclose the same terminal records distinctly."""
    document = reporting_v3.report(_complete_ledger(), "apply", 9)
    reporting_v3.write_json(document)
    encoded = capsys.readouterr()
    assert encoded.out.startswith('{"batch_error":')
    assert "source π display" in encoded.out
    assert "\\u03c0" not in encoded.out
    assert encoded.out.endswith("\n")
    decoded = json.loads(encoded.out)
    assert decoded == json.loads(json.dumps(document))
    assert decoded["batch_error"]["mime_path"] == [9]
    assert decoded["items"][0]["transformation"]["retained"][0]["source_mime_path"] == [
        1,
        3,
    ]
    assert not encoded.err

    reporting_v3.write_human(document)
    human = capsys.readouterr()
    assert human.out == "created: source π display\nfailed: failed display\n"
    assert human.err == (
        "source π display: CREATED_WARN: created warning\n"
        "failed display: FAILED_WARN: failed warning\n"
        "failed display: PARSE_ERROR: failed message\n"
    )


def test_complete_receipt_paths0_emits_only_published_native_address(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """The binary channel is precise while warning and failure receipts stay visible."""
    reporting_v3.write_paths0(_complete_ledger())
    output, errors = capfd.readouterr()
    assert output.encode() == b"published.bin\0"
    assert errors == (
        "source π display: CREATED_WARN: created warning\n"
        "failed display: PARSE_ERROR: failed message\n"
        "failed display: FAILED_WARN: failed warning\n"
    )

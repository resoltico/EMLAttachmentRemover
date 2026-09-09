"""Schema-3 report serialization and the three supported output channels."""

from __future__ import annotations

import json
import os
import sys
from base64 import b64decode, b64encode
from typing import TextIO

from ._version import PROGRAM_VERSION
from .domain import (
    PROGRAM_NAME,
    SCHEMA_VERSION,
    SCOPE,
    AppError,
    BatchLedger,
    InterruptionRecord,
    ItemStatus,
    LedgerItem,
    PathValue,
)


def _path(value: PathValue | None) -> dict[str, str | None] | None:
    if value is None:
        return None
    return {
        "text": value.text,
        "display": value.display,
        "native_base64": value.native_base64,
        "native_utf16le_base64": value.native_utf16le_base64,
    }


def _basename(value: bytes | str) -> dict[str, str | None]:
    """Serialize an exact basename in its one native platform representation.

    Returns:
        Both schema fields with the inapplicable native representation set to null.

    """
    if isinstance(value, bytes):
        return {
            "basename_base64": b64encode(value).decode("ascii"),
            "basename_utf16le_base64": None,
        }
    return {
        "basename_base64": None,
        "basename_utf16le_base64": b64encode(
            value.encode("utf-16-le", "strict")
        ).decode("ascii"),
    }


def _error(error: AppError | None) -> dict[str, object] | None:
    if error is None:
        return None
    code = error.code
    return {
        "code": code.name,
        "message": error.message,
        "mime_path": error.mime_path,
        "phase": error.phase,
    }


def _interruption(record: InterruptionRecord | None) -> dict[str, str] | None:
    if record is None:
        return None
    return {"signal": record.signal, "reason": record.reason, "phase": record.phase}


def _source(item: LedgerItem) -> dict[str, object] | None:
    source = item.source
    if source is None:
        return None
    return {
        "expanded": _path(source.expanded),
        "parent": _path(source.parent),
        **_basename(source.basename),
        "final_address": _path(source.final_address),
        "identity": source.identity.as_json(),
        "sha256": source.digest,
        "size": source.size,
    }


def _destination(item: LedgerItem) -> dict[str, object] | None:
    destination = item.destination
    if destination is None:
        return None
    return {
        "parent": _path(destination.parent),
        **_basename(destination.basename),
        "final_address": _path(destination.request),
        "identity": destination.directory_identity.as_json(),
    }


def _transformation(item: LedgerItem) -> dict[str, object] | None:
    plan = item.transformation
    if plan is None:
        return None
    return {
        "retained": [
            {
                "source_mime_path": fingerprint.source_path,
                "content_type": fingerprint.content_type,
                "cte": fingerprint.cte,
                "content_type_parameters": [
                    {
                        "name_base64": b64encode(name).decode("ascii"),
                        "value_base64": b64encode(value).decode("ascii"),
                    }
                    for name, value in fingerprint.content_type_parameters
                ],
                "encoded_sha256": fingerprint.encoded_sha256,
                "decoded_sha256": fingerprint.decoded_sha256,
            }
            for fingerprint in plan.retained
        ],
        "removal_roots": [
            {
                "mime_path": removal.path,
                "content_type": removal.content_type,
                "reason": removal.reason,
            }
            for removal in plan.removals
        ],
        "stripped_headers": list(plan.stripped_headers),
        "candidate_sha256": plan.candidate_sha256,
        "candidate_size": plan.candidate_size,
    }


def _verification(item: LedgerItem) -> dict[str, bool] | None:
    receipt = item.verification
    if receipt is None:
        return None
    return {
        "output_parses": receipt.output_parses,
        "retained_payloads_match": receipt.retained_payloads_match,
        "structure_matches": receipt.structure_matches,
        "policy_is_idempotent": receipt.policy_is_idempotent,
        "digest_matches": receipt.digest_matches,
    }


def _publication(item: LedgerItem) -> dict[str, object] | None:
    receipt = item.publication
    if receipt is None:
        return None
    return {
        "visibility": receipt.visibility,
        "identity": None if receipt.identity is None else receipt.identity.as_json(),
        "sha256": receipt.digest,
        "file_sync": receipt.file_sync,
        "directory_sync": receipt.directory_sync,
        "address_verified": receipt.address_verified,
        "final_address": _path(receipt.final_address),
        "temp_cleanup": receipt.temp_cleanup,
    }


def item_json(item: LedgerItem) -> dict[str, object]:
    """Serialize one complete terminal item record.

    Returns:
        The schema-3 mapping for the one terminal ledger record.

    """
    return {
        "index": item.index,
        "phase": item.phase,
        "status": item.status,
        "terminalized": item.terminalized,
        "source_request": _path(item.source_request),
        "destination_request": _path(item.destination_request),
        "source": _source(item),
        "destination": _destination(item),
        "transformation": _transformation(item),
        "verification": _verification(item),
        "publication": _publication(item),
        "warnings": item.warnings,
        "error": _error(item.error),
    }


def report(ledger: BatchLedger, mode: str, exit_code: int) -> dict[str, object]:
    """Build the exact top-level schema-3 report document.

    Returns:
        The complete canonical report mapping.

    Raises:
        RuntimeError: If a caller requests a report before ledger terminalization.

    """
    counts = {status.value: 0 for status in ItemStatus}
    for item in ledger.items:
        if item.status is None:
            message = "report requested before ledger terminalization"
            raise RuntimeError(message)
        counts[item.status.value] += 1
    counts["total"] = len(ledger.items)
    accepted = {
        ItemStatus.WOULD_CREATE,
        ItemStatus.CREATED,
        ItemStatus.EXISTING_VERIFIED,
    }
    return {
        "schema_version": SCHEMA_VERSION,
        "scope": SCOPE,
        "program": PROGRAM_NAME,
        "version": PROGRAM_VERSION,
        "mode": mode,
        "ok": all(item.status in accepted for item in ledger.items),
        "exit_code": exit_code,
        "interrupted": ledger.interruption is not None,
        "interruption": _interruption(ledger.interruption),
        "batch_error": _error(ledger.batch_error),
        "summary": counts,
        "items": [item_json(item) for item in ledger.items],
    }


def write_json(document: dict[str, object]) -> None:
    """Write exactly one canonical JSON report document."""
    sys.stdout.write(json.dumps(document, ensure_ascii=False, sort_keys=True) + "\n")


def _safe(stream: TextIO, text: str) -> None:
    """Write one display-safe line."""
    stream.write(
        text.encode(stream.encoding or "utf-8", "backslashreplace").decode(
            stream.encoding or "utf-8"
        )
        + "\n"
    )


def write_human(document: dict[str, object]) -> None:
    """Write a compact display report from a schema document.

    Raises:
        TypeError: If the report items field is not a list.

    """
    items = document["items"]
    if not isinstance(items, list):
        message = "report items are not a list"
        raise TypeError(message)
    for item in items:
        typed = item if isinstance(item, dict) else {}
        request = typed.get("source_request")
        display = request.get("display") if isinstance(request, dict) else "<unknown>"
        _safe(sys.stdout, f"{typed.get('status')}: {display}")
        _write_warnings(display, typed.get("warnings"))
        error = typed.get("error")
        if isinstance(error, dict):
            _safe(sys.stderr, f"{display}: {error.get('code')}: {error.get('message')}")


def _write_warnings(display: object, warnings: object) -> None:
    """Write source-qualified structured warnings to human-output standard error."""
    if not isinstance(warnings, list):
        return
    for warning in warnings:
        if isinstance(warning, dict):
            _safe(
                sys.stderr,
                f"{display}: {warning.get('code')}: {warning.get('message')}",
            )


def write_paths0(ledger: BatchLedger) -> None:
    """Write accepted final paths as exact native bytes and errors to stderr."""
    accepted = {ItemStatus.CREATED, ItemStatus.EXISTING_VERIFIED}
    output = sys.stdout.buffer
    for item in ledger.items:
        receipt = item.publication
        final_address = None if receipt is None else receipt.final_address
        if (
            item.status in accepted
            and final_address is not None
            and final_address.text is not None
        ):
            native = (
                b64decode(final_address.native_base64)
                if final_address.native_base64 is not None
                else os.fsencode(final_address.text)
            )
            output.write(native + b"\0")
        if item.error is not None:
            _safe(
                sys.stderr,
                (
                    f"{item.source_request.display}: {item.error.code.name}: "
                    f"{item.error.message}"
                ),
            )
        _write_warnings(item.source_request.display, item.warnings)

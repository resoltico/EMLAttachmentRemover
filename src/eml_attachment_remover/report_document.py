"""Schema-3 report serialization and the three supported output channels."""

from __future__ import annotations

import json
import sys
from base64 import b64encode
from encodings import utf_8, utf_16_le
from typing import Final, TextIO

from . import report_diagnostics
from ._version import program_version
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
from .native_values import (
    has_surrogate,
    planned_display,
    report_path_bytes,
    safe_display,
)
from .report_batch_diagnostics import batch_error_line, write_batch_error

_UTF8: Final = utf_8.getregentry().name
MAX_ERROR_MESSAGE: Final = report_diagnostics.MAX_ERROR_MESSAGE
MAX_ERROR_BYTES: Final = report_diagnostics.MAX_ERROR_BYTES


def _base64(value: bytes) -> str:
    """Return canonical text for binary schema evidence.

    Returns:
        Canonical UTF-8 text for the Base64 alphabet.

    """
    return b64encode(value).decode()


def _utf16le(value: str) -> bytes:
    """Return strict UTF-16LE evidence without a mutable codec spelling.

    Returns:
        The strict UTF-16LE representation of ``value``.

    """
    return utf_16_le.encode(value)[0]


def canonical_json(document: dict[str, object]) -> str:
    """Serialize one report through the single portable JSON policy.

    Returns:
        One canonical JSON document without a trailing newline.

    """
    return json.dumps(document, ensure_ascii=True, sort_keys=True, allow_nan=False)


def path_json(value: PathValue | None) -> dict[str, str | None] | None:
    """Serialize one native path without deriving an address from display text.

    Returns:
        Its canonical report fields, or None when the value is absent.

    """
    if value is None:
        return None
    text = value.text
    if text is not None and has_surrogate(text):
        text = None
    return {
        "text": text,
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
            "basename_base64": _base64(value),
            "basename_utf16le_base64": None,
        }
    return {
        "basename_base64": None,
        "basename_utf16le_base64": _base64(_utf16le(value)),
    }


def error_json(error: AppError | None) -> dict[str, object] | None:
    """Serialize a bounded expected failure for the report document.

    Returns:
        Its canonical report fields, or None when the value is absent.

    """
    if error is None:
        return None
    code = error.code
    message = report_diagnostics.bounded_message(error.message)
    return {
        "code": code.name,
        "message": message,
        "mime_path": error.mime_path,
        "phase": error.phase,
    }


def interruption_json(record: InterruptionRecord | None) -> dict[str, str] | None:
    """Serialize an invocation interruption independently of item outcomes.

    Returns:
        Its canonical report fields, or None when the value is absent.

    """
    if record is None:
        return None
    return {"signal": record.signal, "reason": record.reason, "phase": record.phase}


def _source(item: LedgerItem) -> dict[str, object] | None:
    source = item.source
    if source is None:
        return None
    return {
        "expanded": path_json(source.expanded),
        "parent": path_json(source.parent),
        **_basename(source.basename),
        "final_address": path_json(source.final_address),
        "identity": source.identity.as_json(),
        "sha256": source.digest,
        "size": source.size,
    }


def _destination(item: LedgerItem) -> dict[str, object] | None:
    destination = item.destination
    if destination is None:
        return None
    return {
        "parent": path_json(destination.parent),
        **_basename(destination.basename),
        "final_address": None,
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
                        "name_base64": _base64(name),
                        "value_base64": _base64(value),
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
        "authorization_matches": receipt.authorization_matches,
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
        "final_address": path_json(receipt.final_address),
        "temp_cleanup": receipt.temp_cleanup,
    }


def item_json(item: LedgerItem) -> dict[str, object]:
    """Serialize one complete terminal item record.

    Returns:
        The schema-3 mapping for the one terminal ledger record.

    """
    item = item.completed_view()
    return {
        "index": item.index,
        "phase": item.phase,
        "status": item.status,
        "terminalized": item.terminalized,
        "source_request": path_json(item.source_request),
        "destination_request": path_json(item.destination_request),
        "source": _source(item),
        "destination": _destination(item),
        "transformation": _transformation(item),
        "verification": _verification(item),
        "publication": _publication(item),
        "warnings": item.warnings,
        "error": error_json(item.error),
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
        if item.status is None or not item.terminalized:
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
        "version": program_version(),
        "mode": mode,
        "ok": ledger.batch_error is None
        and all(item.status in accepted for item in ledger.items),
        "exit_code": exit_code,
        "interrupted": ledger.interruption is not None,
        "interruption": interruption_json(ledger.interruption),
        "batch_error": error_json(ledger.batch_error),
        "summary": counts,
        "items": [item_json(item) for item in ledger.items],
    }


def write_json(document: dict[str, object]) -> None:
    """Write exactly one canonical JSON report document."""
    payload = (canonical_json(document) + "\n").encode("ascii")
    binary = getattr(sys.stdout, "buffer", None)
    if binary is None:
        sys.stdout.write(payload.decode("ascii"))
    else:
        binary.write(payload)


def write_display(stream: TextIO, text: str) -> None:
    """Write one display-safe line."""
    encoding = stream.encoding or _UTF8
    escaped = safe_display(text).encode(encoding, "backslashreplace")
    stream.write(escaped.decode(encoding) + "\n")


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
        target = _target_display(typed)
        arrow = "" if target is None else f" -> {target}"
        write_display(sys.stdout, f"{typed.get('status')}: {display}{arrow}")
        _write_warnings(display, typed.get("warnings"))
        error = typed.get("error")
        if isinstance(error, dict):
            write_display(
                sys.stderr, f"{display}: {error.get('code')}: {error.get('message')}"
            )
    if line := batch_error_line(document.get("batch_error")):
        sys.stderr.write(line)


def _target_display(record: dict[str, object]) -> str | None:
    """Return where an item's copy is, or would be, for human output.

    Returns:
        The receipt-proven final address, else a dry run's planned destination,
        else ``None`` when no copy exists or is planned.

    """
    publication = record.get("publication")
    final = publication.get("final_address") if isinstance(publication, dict) else None
    if isinstance(final, dict):
        return str(final.get("display"))
    destination = record.get("destination")
    if record.get("status") != ItemStatus.WOULD_CREATE.value or not isinstance(
        destination, dict
    ):
        return None
    return planned_display(destination)


def _write_warnings(display: object, warnings: object) -> None:
    """Write source-qualified structured warnings to human-output standard error."""
    if not isinstance(warnings, list):
        return
    for warning in warnings:
        if isinstance(warning, dict):
            write_display(
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
        if item.status in accepted and final_address is not None:
            native = report_path_bytes(final_address)
            output.write(native + b"\0")
        if item.error is not None:
            write_display(
                sys.stderr,
                (
                    f"{item.source_request.display}: {item.error.code.name}: "
                    f"{item.error.message}"
                ),
            )
        _write_warnings(item.source_request.display, item.warnings)
    write_batch_error(ledger.batch_error)

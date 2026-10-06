#!/bin/sh
# Owned processing and validated JSON transport for the native application.

set -u

PROGRAM_NAME="EML Attachment Remover"
ZIPAPP=${EML_REMOVER_ZIPAPP:-}

fail() {
    code=$1
    shift
    printf '%s: %s\n' "$PROGRAM_NAME" "$*" >&2
    case "$code" in
        9) kind=python_unavailable ;;
        3) kind=processor_unavailable ;;
        7) kind=report_storage_unavailable ;;
        *) kind=invalid_request ;;
    esac
    printf '{"launcher_error":"%s","process_status":%s}\n' "$kind" "$code"
    exit "$code"
}

find_python() {
    if [ -n "${EML_REMOVER_PYTHON:-}" ]; then
        [ -x "$EML_REMOVER_PYTHON" ] || fail 9 "the selected CPython 3.14 runtime is missing or not executable"
        PYTHON=$EML_REMOVER_PYTHON
        return 0
    fi
    PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.local/bin:$HOME/.pyenv/shims:$PATH"
    export PATH
    PYTHON=$(command -v python3.14 2>/dev/null) || return 1
}

[ "$#" -gt 0 ] || fail 2 "no Finder files were supplied"
[ -f "$ZIPAPP" ] && [ ! -L "$ZIPAPP" ] || fail 3 "bundled processor is missing or unsafe"
PYTHON=
find_python || fail 9 "Python 3.14 was not found; set EML_REMOVER_PYTHON"
"$PYTHON" -I -B -c 'import platform, sys; raise SystemExit(platform.python_implementation() != "CPython" or sys.version_info[:2] != (3, 14))' || fail 9 "the selected interpreter is not CPython 3.14"

case ${EML_REMOVER_EXISTING:-verify} in
    error|verify) existing=${EML_REMOVER_EXISTING:-verify} ;;
    *) fail 2 "EML_REMOVER_EXISTING must be error or verify" ;;
esac

umask 077
REPORT_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-report.XXXXXX") || fail 7 "could not create report file"
ERROR_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-errors.XXXXXX") || { rm -f "$REPORT_FILE"; fail 7 "could not create error file"; }
CHILD=
WATCHDOG=
OWNER_WATCHER=
CANCEL_STATUS=0
CANCEL_SIGNAL=
# shellcheck disable=SC2329  # Invoked through the EXIT trap.
cleanup() {
    if [ -n "$WATCHDOG" ]; then
        kill "$WATCHDOG" 2>/dev/null || true
        wait "$WATCHDOG" 2>/dev/null || true
    fi
    if [ -n "$OWNER_WATCHER" ]; then
        kill "$OWNER_WATCHER" 2>/dev/null || true
        wait "$OWNER_WATCHER" 2>/dev/null || true
    fi
    rm -f "$REPORT_FILE" "$ERROR_FILE"
}
# shellcheck disable=SC2329  # Invoked through signal traps.
forward() {
    signal=$1
    requested_status=$2
    if [ "$CANCEL_STATUS" -ne 0 ]; then
        if [ -n "$CHILD" ]; then kill -KILL "$CHILD" 2>/dev/null || true; fi
        return
    fi
    CANCEL_STATUS=$requested_status
    CANCEL_SIGNAL=$signal
    interrupt_child
}
# shellcheck disable=SC2329  # Also delivers a signal caught before child startup.
interrupt_child() {
    if [ -n "$WATCHDOG" ]; then return; fi
    if [ -n "$CHILD" ]; then
        kill "-$CANCEL_SIGNAL" "$CHILD" 2>/dev/null || true
        "$PYTHON" -I -B -c 'import os, signal, sys, time; time.sleep(12); os.kill(int(sys.argv[1]), signal.SIGKILL)' "$CHILD" 2>/dev/null &
        WATCHDOG=$!
    fi
}

trap cleanup EXIT
trap 'forward HUP 129' HUP
trap 'forward INT 130' INT
trap 'forward TERM 143' TERM

# The native app alone holds this pipe's write end. EOF also covers forced app loss.
if [ "${EML_REMOVER_UI_OWNER_PIPE:-0}" = 1 ]; then
    # Preserve the live input before POSIX shells redirect background stdin.
    exec 3<&0
    "$PYTHON" -I -B -c '
import os, select, signal, sys
parent = int(sys.argv[1])
if hasattr(select, "kqueue"):
    monitor = select.kqueue()
    monitor.control([select.kevent(0, filter=select.KQ_FILTER_READ, flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR)], 0, 0)
    while not any(event.flags & select.KQ_EV_EOF for event in monitor.control(None, 1, 1)):
        if os.getppid() != parent: raise SystemExit(0)
elif hasattr(select, "poll"):
    monitor = select.poll()
    monitor.register(0, select.POLLHUP | select.POLLERR)
    while not monitor.poll(1000):
        if os.getppid() != parent: raise SystemExit(0)
else:
    # The native macOS adapter always uses kqueue; this fallback serves plain
    # launcher ownership probes on hosts without a non-consuming pipe observer.
    sys.stdin.buffer.read()
if os.getppid() == parent: os.kill(parent, signal.SIGTERM)
' "$$" <&3 3<&- >/dev/null 2>/dev/null &
    OWNER_WATCHER=$!
    exec 3<&-
fi

exec 3>&2
exec 4</dev/null
if [ "${EML_REMOVER_UI_REQUEST_PIPE:-0}" = 1 ]; then
    # The processor reads the framed selection and then owns this pipe's lifetime.
    # No second reader may consume request bytes or race its EOF monitor.
    exec 4<&0
    set -- --request-stdin
else
    set -- -- "$@"
fi
if [ "${EML_REMOVER_UI_PROGRESS_PIPE:-0}" = 1 ]; then
    set -- --progress-fd=3 "$@"
fi
if [ -n "${EML_REMOVER_OUTPUT_DIR:-}" ]; then
    set -- "--output-dir=$EML_REMOVER_OUTPUT_DIR" "$@"
fi
"$PYTHON" -I -B "$ZIPAPP" --existing="$existing" --output-format json "$@" <&4 4<&- >"$REPORT_FILE" 2>"$ERROR_FILE" &
CHILD=$!
exec 3>&-
exec 4<&-
if [ "$CANCEL_STATUS" -ne 0 ]; then interrupt_child; fi
while :; do
    wait "$CHILD"
    status=$?
    # A caught signal can interrupt wait before the processor has exited.
    if ! kill -0 "$CHILD" 2>/dev/null; then break; fi
done
CHILD=
if [ -n "$WATCHDOG" ]; then
    kill "$WATCHDOG" 2>/dev/null || true
    wait "$WATCHDOG" 2>/dev/null || true
    WATCHDOG=
fi
"$PYTHON" -I -B - "$REPORT_FILE" "$ERROR_FILE" "$status" "$CANCEL_STATUS" <<'PY'
from __future__ import annotations

import base64
import binascii
import json
import os
import sys
import unicodedata
from collections.abc import Mapping

report_path, error_path, processor_status, cancellation_status = sys.argv[1:]
STATUSES = (
    "created",
    "existing_verified",
    "would_create",
    "failed",
    "cancelled",
    "not_run",
    "published_with_error",
)
TOP_LEVEL_FIELDS = {
    "schema_version", "scope", "program", "version", "mode", "ok", "exit_code",
    "interrupted", "interruption", "batch_error", "summary", "items",
}
ITEM_FIELDS = {
    "index", "phase", "status", "terminalized", "source_request",
    "destination_request", "source", "destination", "transformation", "verification",
    "publication", "warnings", "error",
}
MAX_TEXT = 512
INTERRUPTED = 130
OUTPUT_FINALIZATION = 120


def safe(value: object) -> str:
    text = value if isinstance(value, str) else "<invalid>"
    rendered = "".join(
        character
        if unicodedata.category(character) not in {"Cc", "Cf", "Cs", "Zl", "Zp"}
        else f"\\u{ord(character):04x}"
        if ord(character) <= 0xffff
        else f"\\U{ord(character):08x}"
        for character in text
    )
    return rendered if len(rendered) <= MAX_TEXT else rendered[:MAX_TEXT] + "…"


def mapping(value: object, label: str) -> Mapping[str, object]:
    if not isinstance(value, Mapping):
        raise ValueError(f"{label} is not an object")
    return value


def error_detail(value: object, label: str) -> tuple[str, str] | None:
    if value is None:
        return None
    detail = mapping(value, label)
    if not isinstance(detail.get("code"), str) or not isinstance(detail.get("message"), str):
        raise ValueError(f"{label} is malformed")
    return safe(detail["code"]), safe(detail["message"])


def accepted_path(final: Mapping[str, object]) -> str:
    native = final.get("native_base64")
    text = final.get("text")
    if isinstance(native, str):
        try:
            raw = base64.b64decode(native, validate=True)
        except (binascii.Error, ValueError) as exc:
            raise ValueError("accepted native path is malformed") from exc
        if b"\0" in raw:
            raise ValueError("accepted native path contains NUL")
        address = os.fsdecode(raw)
        if isinstance(text, str) and os.fsencode(text) != raw:
            raise ValueError("accepted native/text path evidence disagrees")
        if text is not None and not isinstance(text, str):
            raise ValueError("accepted path text is malformed")
        return address
    if not isinstance(text, str):
        raise ValueError("accepted output lacks native path evidence")
    return text


def validate(report: object, status: int) -> list[str]:
    document = mapping(report, "report")
    if set(document) != TOP_LEVEL_FIELDS:
        raise ValueError("unexpected report fields")
    if (
        document.get("schema_version") != 3
        or document.get("scope") != "mime-pruned"
        or document.get("program") != "remove-eml-attachments"
        or not isinstance(document.get("version"), str)
        or document.get("mode") not in {"apply", "dry-run"}
        or type(document.get("ok")) is not bool
        or type(document.get("exit_code")) is not int
        # Late cancellation or CPython output-finalization failure preserves a
        # complete document's processing outcome while the process status differs.
        or (document["exit_code"] != status and status not in {INTERRUPTED, OUTPUT_FINALIZATION} and cancellation_status == "0")
    ):
        raise ValueError("report identity or exit status is invalid")
    items = document.get("items")
    summary = mapping(document.get("summary"), "report summary")
    if not isinstance(items, list) or set(summary) != {*STATUSES, "total"}:
        raise ValueError("report items or summary is invalid")
    counts = {name: 0 for name in STATUSES}
    details: list[str] = []
    for index, raw_item in enumerate(items):
        item = mapping(raw_item, "report item")
        if (
            set(item) != ITEM_FIELDS
            or item.get("index") != index
            or item.get("status") not in STATUSES
            or item.get("terminalized") is not True
        ):
            raise ValueError("report item receipt is invalid")
        item_status = item["status"]
        counts[item_status] += 1
        error_detail(item.get("error"), "item error")
        warnings = item.get("warnings")
        if not isinstance(warnings, list):
            raise ValueError("item warnings are invalid")
        for warning in warnings:
            detail = error_detail(warning, "item warning")
            if detail is None:
                raise ValueError("item warning is invalid")
        if item_status in {"created", "existing_verified"}:
            publication = mapping(item.get("publication"), "accepted publication")
            final = mapping(publication.get("final_address"), "accepted final address")
            if (
                publication.get("visibility") not in {"visible", "existing_verified"}
                or publication.get("address_verified") is not True
                or final.get("native_utf16le_base64") is not None
            ):
                raise ValueError("accepted output lacks a verified final address")
            accepted_path(final)
    if (
        any(type(summary.get(name)) is not int or summary[name] != counts[name] for name in STATUSES)
        or type(summary.get("total")) is not int
        or summary["total"] != len(items)
    ):
        raise ValueError("report summary does not match item receipts")
    accepted = {"created", "existing_verified"}
    if document["mode"] == "dry-run":
        accepted.add("would_create")
    batch_error = error_detail(document.get("batch_error"), "batch error")
    if document["ok"] is not (
        batch_error is None
        and all(item["status"] in accepted for item in (mapping(value, "report item") for value in items))
    ):
        raise ValueError("report ok value does not match item receipts")
    if batch_error is not None:
        details.append(f"Batch: {batch_error[0]}: {batch_error[1]}")
    interrupted = document.get("interrupted")
    interruption = document.get("interruption")
    if (
        type(interrupted) is not bool
        or (interrupted and not isinstance(interruption, Mapping))
        or (not interrupted and interruption is not None)
    ):
        raise ValueError("report interruption receipt is invalid")
    if interrupted:
        details.append(f"Interrupted: {safe(interruption.get('reason'))}")
    if status == OUTPUT_FINALIZATION:
        details.insert(0, "Output finalization failed: processor status 120; available results are complete.")
    if cancellation_status != "0":
        details.append("Interrupted: launcher cancelled; available results are complete.")
    elif document["exit_code"] != status and status == INTERRUPTED:
        details.append("Interrupted: after the report was written; its results are complete.")
    return details


try:
    with open(report_path, encoding="utf-8") as report_source:
        report = json.load(report_source)
    details = validate(report, int(processor_status))
except (OSError, ValueError, json.JSONDecodeError) as exc:
    print(f"EML Attachment Remover: invalid processor report: {safe(str(exc))}")
    print("Processor diagnostics were withheld because the canonical report was invalid.")
    raise SystemExit(70)

final_status = int(cancellation_status) or int(processor_status)
print(json.dumps({
    "report": report,
    "process_status": final_status,
    "details": details,
}, ensure_ascii=True))
raise SystemExit(final_status)
PY
validation_status=$?
if [ "$CANCEL_STATUS" -ne 0 ]; then exit "$CANCEL_STATUS"; fi
exit "$validation_status"

#!/bin/sh
# Finder/Shortcuts launcher for the schema-3 MIME-pruned zipapp.

set -u

PROGRAM_NAME="EML Attachment Remover"
INSTALL_DIR=${EML_REMOVER_HOME:-"$HOME/Library/Application Support/$PROGRAM_NAME"}
ZIPAPP=${EML_REMOVER_ZIPAPP:-"$INSTALL_DIR/remove-eml-attachments.pyz"}

fail() {
    code=$1
    shift
    printf '%s: %s\n' "$PROGRAM_NAME" "$*" >&2
    exit "$code"
}

find_python() {
    if [ -n "${EML_REMOVER_PYTHON:-}" ] && [ -x "$EML_REMOVER_PYTHON" ]; then
        PYTHON=$EML_REMOVER_PYTHON
        return 0
    fi
    PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.local/bin:$HOME/.pyenv/shims:$PATH"
    export PATH
    PYTHON=$(command -v python3.14 2>/dev/null) || return 1
}

[ "$#" -gt 0 ] || fail 2 "no Finder files were supplied"
[ -f "$ZIPAPP" ] && [ ! -L "$ZIPAPP" ] || fail 3 "zipapp is missing or unsafe; run install.sh first"
PYTHON=
find_python || fail 9 "Python 3.14 was not found; set EML_REMOVER_PYTHON"
"$PYTHON" -c 'import platform, sys; raise SystemExit(platform.python_implementation() != "CPython" or sys.version_info[:2] != (3, 14))' || fail 9 "the selected interpreter is not CPython 3.14"

case ${EML_REMOVER_EXISTING:-verify} in
    error|verify) existing=${EML_REMOVER_EXISTING:-verify} ;;
    skip|force) fail 2 "EML_REMOVER_EXISTING=skip|force was removed in v3; use error or verify" ;;
    *) fail 2 "EML_REMOVER_EXISTING must be error or verify" ;;
esac

umask 077
REPORT_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-report.XXXXXX") || fail 7 "could not create report file"
ERROR_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-errors.XXXXXX") || { rm -f "$REPORT_FILE"; fail 7 "could not create error file"; }
CHILD=
# shellcheck disable=SC2329  # Invoked through the EXIT trap.
cleanup() { rm -f "$REPORT_FILE" "$ERROR_FILE"; }
# shellcheck disable=SC2329  # Invoked through signal traps.
forward() {
    signal=$1
    status=$2
    if [ -n "$CHILD" ]; then
        kill "-$signal" "$CHILD" 2>/dev/null || true
        wait "$CHILD" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'forward HUP 129' HUP
trap 'forward INT 130' INT
trap 'forward TERM 143' TERM

"$PYTHON" "$ZIPAPP" --existing="$existing" --output-format json -- "$@" >"$REPORT_FILE" 2>"$ERROR_FILE" &
CHILD=$!
wait "$CHILD"
status=$?
CHILD=

"$PYTHON" - "$REPORT_FILE" "$ERROR_FILE" "${EML_REMOVER_REVEAL:-1}" "$status" <<'PY'
from __future__ import annotations

import json
import os
import subprocess
import sys
import unicodedata
from collections.abc import Mapping

report_path, error_path, reveal, processor_status = sys.argv[1:]
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
MAX_DETAILS = 24
MAX_TEXT = 512


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


def item_display(item: Mapping[str, object]) -> str:
    source = mapping(item.get("source_request"), "item source request")
    return safe(source.get("display"))


def validate(report: object, status: int) -> tuple[list[str], list[str]]:
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
        or document["exit_code"] != status
    ):
        raise ValueError("report identity or exit status is invalid")
    items = document.get("items")
    summary = mapping(document.get("summary"), "report summary")
    if not isinstance(items, list) or set(summary) != {*STATUSES, "total"}:
        raise ValueError("report items or summary is invalid")
    counts = {name: 0 for name in STATUSES}
    details: list[str] = []
    outputs: list[str] = []
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
        display = item_display(item)
        item_error = error_detail(item.get("error"), "item error")
        if item_error is not None:
            details.append(f"{display}: {item_error[0]}: {item_error[1]}")
        warnings = item.get("warnings")
        if not isinstance(warnings, list):
            raise ValueError("item warnings are invalid")
        for warning in warnings:
            detail = error_detail(warning, "item warning")
            if detail is None:
                raise ValueError("item warning is invalid")
            details.append(f"{display}: {detail[0]}: {detail[1]}")
        if item_status in {"created", "existing_verified"}:
            publication = mapping(item.get("publication"), "accepted publication")
            final = mapping(publication.get("final_address"), "accepted final address")
            if (
                publication.get("visibility") not in {"visible", "existing_verified"}
                or publication.get("address_verified") is not True
                or not isinstance(final.get("text"), str)
            ):
                raise ValueError("accepted output lacks a verified final address")
            outputs.append(final["text"])
    if (
        any(type(summary.get(name)) is not int or summary[name] != counts[name] for name in STATUSES)
        or type(summary.get("total")) is not int
        or summary["total"] != len(items)
    ):
        raise ValueError("report summary does not match item receipts")
    accepted = {"created", "existing_verified"}
    if document["mode"] == "dry-run":
        accepted.add("would_create")
    if document["ok"] is not all(item["status"] in accepted for item in (mapping(value, "report item") for value in items)):
        raise ValueError("report ok value does not match item receipts")
    batch_error = error_detail(document.get("batch_error"), "batch error")
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
    return outputs, details


try:
    with open(report_path, encoding="utf-8") as report_source:
        report = json.load(report_source)
    outputs, details = validate(report, int(processor_status))
except (OSError, ValueError, json.JSONDecodeError) as exc:
    print(f"EML Attachment Remover: invalid processor report: {safe(str(exc))}")
    print("Processor diagnostics were withheld because the canonical report was invalid.")
    raise SystemExit(70)

summary = report["summary"]
print(
    "MIME-pruned EML: "
    f"created {summary['created']}; existing verified {summary['existing_verified']}; "
    f"failed {summary['failed']}; not run {summary['not_run']}; "
    f"published with error {summary['published_with_error']}; cancelled {summary['cancelled']}."
)
for detail in details[:MAX_DETAILS]:
    print(detail)
if len(details) > MAX_DETAILS:
    print(f"{len(details) - MAX_DETAILS} additional diagnostic(s) were omitted.")
if reveal != "0":
    for output in outputs:
        subprocess.run(["open", "-R", output], check=False)
raise SystemExit(int(processor_status))
PY
exit $?

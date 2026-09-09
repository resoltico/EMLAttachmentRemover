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
from collections.abc import Mapping

report_path, error_path, reveal, processor_status = sys.argv[1:]
try:
    report = json.load(open(report_path, encoding="utf-8"))
    if not isinstance(report, Mapping):
        raise ValueError("report is not an object")
    required = {"schema_version", "scope", "items", "ok", "exit_code"}
    if not required <= set(report) or report["schema_version"] != 3 or report["scope"] != "mime-pruned":
        raise ValueError("unexpected report schema")
    outputs: list[str] = []
    for item in report["items"]:
        if not isinstance(item, Mapping):
            raise ValueError("invalid item")
        if item.get("status") not in {"created", "existing_verified"}:
            continue
        publication = item.get("publication")
        if not isinstance(publication, Mapping) or publication.get("address_verified") is not True:
            raise ValueError("accepted output lacks an address receipt")
        final = publication.get("final_address")
        if not isinstance(final, Mapping) or not isinstance(final.get("text"), str):
            raise ValueError("accepted output lacks a final path")
        outputs.append(final["text"])
except (OSError, ValueError, json.JSONDecodeError) as exc:
    print(f"EML Attachment Remover: invalid processor report: {exc}", file=sys.stderr)
    print(open(error_path, encoding="utf-8", errors="replace").read(), file=sys.stderr)
    raise SystemExit(70)

errors = open(error_path, encoding="utf-8", errors="replace").read()
if errors:
    print(errors, file=sys.stderr, end="" if errors.endswith("\n") else "\n")
if outputs:
    print(f"Created or verified {len(outputs)} MIME-pruned EML output(s).")
    if reveal != "0":
        for output in outputs:
            subprocess.run(["open", "-R", output], check=False)
else:
    print("No MIME-pruned EML output was accepted.")
raise SystemExit(int(processor_status))
PY
exit $?

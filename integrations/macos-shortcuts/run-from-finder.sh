#!/bin/sh
# Finder/Shortcuts launcher for the dependency-free Python zipapp.

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
    if [ -n "${EML_REMOVER_PYTHON:-}" ]; then
        if [ -x "$EML_REMOVER_PYTHON" ]; then
            PYTHON=$EML_REMOVER_PYTHON
            return 0
        fi
        return 1
    fi

    PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.local/bin:$HOME/.pyenv/shims:$PATH"
    export PATH
    for candidate in \
        /opt/homebrew/bin/python3.14 \
        /usr/local/bin/python3.14 \
        /Library/Frameworks/Python.framework/Versions/3.14/bin/python3.14 \
        "$HOME/.local/bin/python3.14" \
        "$HOME/.pyenv/shims/python3.14"
    do
        if [ -x "$candidate" ]; then
            PYTHON=$candidate
            return 0
        fi
    done
    PYTHON=$(command -v python3.14 2>/dev/null)
}

[ "$#" -gt 0 ] || fail 2 "no Finder files were supplied"
[ -f "$ZIPAPP" ] && [ ! -L "$ZIPAPP" ] \
    || fail 3 "the zipapp is missing or is not a safe regular file; run install.sh first"
PYTHON=
find_python || fail 9 "Python 3.14 was not found; set EML_REMOVER_PYTHON"
"$PYTHON" -c \
    'import platform, sys; raise SystemExit(platform.python_implementation() != "CPython" or sys.version_info[:2] != (3, 14))' \
    || fail 9 "the selected interpreter is not CPython 3.14"

umask 077
REPORT_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-report.XXXXXX") \
    || fail 7 "could not create a temporary report file"
ERROR_FILE=$(mktemp "${TMPDIR:-/tmp}/eml-remover-errors.XXXXXX") \
    || {
        rm -f "$REPORT_FILE"
        fail 7 "could not create a temporary error file"
    }
# shellcheck disable=SC2329  # Invoked by the EXIT trap.
cleanup() {
    rm -f "$REPORT_FILE" "$ERROR_FILE"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

case ${EML_REMOVER_EXISTING:-error} in
    error)
        "$PYTHON" "$ZIPAPP" --output-format json -- "$@" \
            >"$REPORT_FILE" 2>"$ERROR_FILE"
        status=$?
        ;;
    skip)
        "$PYTHON" "$ZIPAPP" --skip-existing --output-format json -- "$@" \
            >"$REPORT_FILE" 2>"$ERROR_FILE"
        status=$?
        ;;
    force)
        "$PYTHON" "$ZIPAPP" --force --output-format json -- "$@" \
            >"$REPORT_FILE" 2>"$ERROR_FILE"
        status=$?
        ;;
    *)
        fail 2 "EML_REMOVER_EXISTING must be error, skip, or force"
        ;;
esac

"$PYTHON" - \
    "$REPORT_FILE" \
    "$ERROR_FILE" \
    "${EML_REMOVER_REVEAL:-1}" \
    "$status" <<'PY'
from __future__ import annotations

import json
import os
import platform
import re
import subprocess
import sys
import unicodedata
from collections.abc import Mapping
from pathlib import Path
from typing import TextIO

UNSAFE_CATEGORIES = frozenset({"Cc", "Cf", "Cs", "Zl", "Zp"})
PROGRAM = "remove-eml-attachments"
SCHEMA_VERSION = 2
SCOPE = "text-only"
MIME_PATH_PATTERN = re.compile(r"(?:root|[1-9][0-9]*(?:\.[1-9][0-9]*)*)\Z")
OUTER_KEYS = frozenset({
    "errors",
    "ok",
    "program",
    "results",
    "schema_version",
    "scope",
    "skipped",
    "version",
})
RESULT_KEYS = frozenset({
    "destination",
    "discarded_body_representations",
    "discarded_body_resources",
    "dry_run",
    "output_size",
    "removed_attachments",
    "selected_plain_text_bodies",
    "source",
    "source_size",
    "status",
    "warnings",
})
type ResultCounts = tuple[int, int, int, int]
type ValidatedResult = tuple[str, str, list[str], ResultCounts]


def safe_text(value: str, stream: TextIO) -> str:
    def safe_character(character: str) -> str:
        code_point = ord(character)
        if unicodedata.category(character) not in UNSAFE_CATEGORIES:
            return character
        if code_point <= 0xFF:
            return f"\\x{code_point:02x}"
        if code_point <= 0xFFFF:
            return f"\\u{code_point:04x}"
        return f"\\U{code_point:08x}"

    escaped = "".join(safe_character(character) for character in value)
    encoding = stream.encoding or sys.getdefaultencoding()
    return escaped.encode(encoding, errors="backslashreplace").decode(encoding)


def write_line(stream: TextIO, value: str) -> None:
    print(safe_text(value, stream), file=stream)


def record_list(report: Mapping[str, object], name: str) -> list[Mapping[str, object]]:
    value = report.get(name)
    if not isinstance(value, list) or not all(
        isinstance(item, Mapping) for item in value
    ):
        raise ValueError
    return value


def optional_text(value: object) -> str | None:
    if value is None or isinstance(value, str):
        return value
    raise ValueError


def validate_outer_report(report: Mapping[str, object]) -> None:
    if frozenset(report) != OUTER_KEYS:
        raise ValueError
    if not isinstance(report.get("ok"), bool):
        raise ValueError
    if report.get("program") != PROGRAM:
        raise ValueError
    if report.get("schema_version") != SCHEMA_VERSION:
        raise ValueError
    if report.get("scope") != SCOPE:
        raise ValueError
    version = report.get("version")
    if not isinstance(version, str) or not version:
        raise ValueError


def output_destination(item: Mapping[str, object], status: str) -> str | None:
    if item.get("status") != status:
        raise ValueError
    source = item.get("source")
    if not isinstance(source, str):
        raise ValueError
    destination = optional_text(item.get("destination"))
    if destination is not None and "\0" in destination:
        raise ValueError
    if destination is not None:
        os.fsencode(destination)
    return destination


def require_integer(value: object) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise ValueError
    return value


def require_mime_path(value: object) -> str:
    if not isinstance(value, str) or MIME_PATH_PATTERN.fullmatch(value) is None:
        raise ValueError
    return value


def validate_body_record(item: Mapping[str, object]) -> None:
    expected = {"content_type", "mime_path"}
    if set(item) != expected:
        raise ValueError
    content_type = item.get("content_type")
    if not isinstance(content_type, str) or not content_type:
        raise ValueError
    require_mime_path(item.get("mime_path"))


def validate_file_record(item: Mapping[str, object], *, resource: bool) -> None:
    expected = {"content_type", "disposition", "filename", "mime_path"}
    if resource:
        expected.add("referenced_by")
    if set(item) != expected:
        raise ValueError
    content_type = item.get("content_type")
    if not isinstance(content_type, str) or not content_type:
        raise ValueError
    require_mime_path(item.get("mime_path"))
    optional_text(item.get("disposition"))
    optional_text(item.get("filename"))
    if resource:
        references = item.get("referenced_by")
        if not isinstance(references, list):
            raise ValueError
        for reference in references:
            require_mime_path(reference)


def validate_result(item: Mapping[str, object]) -> ValidatedResult:
    if frozenset(item) != RESULT_KEYS:
        raise ValueError
    destination = output_destination(item, "ok")
    if destination is None or item.get("dry_run") is not False:
        raise ValueError
    if not Path(destination).name.casefold().endswith(".text-only.eml"):
        raise ValueError
    require_integer(item.get("source_size"))
    require_integer(item.get("output_size"))
    source = item.get("source")
    if not isinstance(source, str):
        raise ValueError
    warnings = item.get("warnings")
    if not isinstance(warnings, list) or not all(
        isinstance(warning, str) for warning in warnings
    ):
        raise ValueError
    selected = record_list(item, "selected_plain_text_bodies")
    if len(selected) != 1:
        raise ValueError
    validate_body_record(selected[0])
    if selected[0].get("content_type") != "text/plain":
        raise ValueError
    representations = record_list(item, "discarded_body_representations")
    for record in representations:
        validate_body_record(record)
    resources = record_list(item, "discarded_body_resources")
    for record in resources:
        validate_file_record(record, resource=True)
    attachments = record_list(item, "removed_attachments")
    for record in attachments:
        validate_file_record(record, resource=False)
    return (
        destination,
        source,
        warnings,
        (
            len(selected),
            len(representations),
            len(resources),
            len(attachments),
        ),
    )


def validate_skip(item: Mapping[str, object]) -> str:
    if frozenset(item) != {"destination", "source", "status"}:
        raise ValueError
    destination = output_destination(item, "skipped")
    if destination is None:
        raise ValueError
    return destination


def error_line(item: Mapping[str, object]) -> str:
    if frozenset(item) != {"error", "source", "status"}:
        raise ValueError
    if item.get("status") != "error":
        raise ValueError
    source = optional_text(item.get("source")) or "<request>"
    error = item.get("error")
    if not isinstance(error, Mapping):
        raise ValueError
    if frozenset(error) != {"code", "message", "name"}:
        raise ValueError
    name = error.get("name")
    code = error.get("code")
    message = error.get("message")
    if not isinstance(name, str) or not isinstance(code, int) or isinstance(code, bool):
        raise ValueError
    if not isinstance(message, str):
        raise ValueError
    return f"{source}: {name} ({code}): {message}"


def reveal_output(output: str, *, reveal: bool) -> None:
    if not reveal or platform.system() != "Darwin":
        return
    try:
        subprocess.run(
            ["/usr/bin/open", "-R", "--", output],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        pass


def write_diagnostics(diagnostics: str) -> None:
    if not diagnostics:
        return
    lines = diagnostics.split("\n")
    if lines[-1] == "":
        lines.pop()
    for index, line in enumerate(lines):
        prefix = "Processor diagnostics: " if index == 0 else "  "
        write_line(sys.stderr, f"{prefix}{line}")


def main() -> int:
    report_path = Path(sys.argv[1])
    error_path = Path(sys.argv[2])
    reveal = sys.argv[3].casefold() not in {"0", "false", "no", "off"}
    processor_status = int(sys.argv[4])
    diagnostics = error_path.read_bytes().decode("utf-8", errors="backslashreplace")

    report_bytes = report_path.read_bytes()
    if not report_bytes:
        write_diagnostics(diagnostics)
        write_line(sys.stderr, "The processor produced no report.")
        return 70
    try:
        report = json.loads(report_bytes)
        if not isinstance(report, Mapping):
            raise ValueError
        validate_outer_report(report)
        results = record_list(report, "results")
        skipped = record_list(report, "skipped")
        errors = record_list(report, "errors")
        created: list[str] = []
        totals = [0, 0, 0, 0]
        result_warnings: list[str] = []
        for item in results:
            destination, source, warnings, counts = validate_result(item)
            created.append(destination)
            totals = [total + count for total, count in zip(totals, counts, strict=True)]
            result_warnings.extend(f"{source}: {warning}" for warning in warnings)
        skipped_outputs = [validate_skip(item) for item in skipped]
        error_lines = [error_line(item) for item in errors]
        if report["ok"] is not (not errors):
            raise ValueError
        if bool(errors) is (processor_status == 0):
            raise ValueError
    except (json.JSONDecodeError, UnicodeError, ValueError):
        write_diagnostics(diagnostics)
        raise

    write_diagnostics(diagnostics)

    if created:
        noun = "file" if len(created) == 1 else "files"
        write_line(
            sys.stdout,
            f"Created {len(created)} verified text-only EML {noun}:",
        )
        for output in created:
            write_line(sys.stdout, output)
            reveal_output(output, reveal=reveal)
        write_line(sys.stdout, "Transformation totals:")
        write_line(sys.stdout, f"  Selected plain-text bodies: {totals[0]}")
        write_line(sys.stdout, f"  Discarded body representations: {totals[1]}")
        write_line(sys.stdout, f"  Discarded body resources: {totals[2]}")
        write_line(sys.stdout, f"  Removed ordinary attachments: {totals[3]}")
    else:
        write_line(sys.stdout, "No verified text-only EML files were created.")

    if skipped_outputs:
        noun = "output" if len(skipped_outputs) == 1 else "outputs"
        pronoun = "it" if len(skipped_outputs) == 1 else "them"
        write_line(
            sys.stdout,
            f"Skipped {len(skipped_outputs)} existing {noun} without changing or "
            f"verifying {pronoun}:",
        )
        for output in skipped_outputs:
            write_line(sys.stdout, output)
            reveal_output(output, reveal=reveal)

    if result_warnings:
        noun = "warning" if len(result_warnings) == 1 else "warnings"
        write_line(sys.stderr, f"{len(result_warnings)} processing {noun}:")
        for warning in result_warnings:
            write_line(sys.stderr, warning)

    if errors:
        write_line(sys.stderr, f"{len(errors)} input file(s) failed:")
        for line in error_lines:
            write_line(sys.stderr, line)
    return 0


try:
    raise SystemExit(main())
except (json.JSONDecodeError, OSError, UnicodeError, ValueError):
    write_line(sys.stderr, "The processor report was invalid and could not be displayed.")
    raise SystemExit(70) from None
PY
parser_status=$?
if [ "$parser_status" -ne 0 ]; then
    status=$parser_status
fi
exit "$status"

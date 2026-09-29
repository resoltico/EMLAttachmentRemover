"""Planned destinations are built natively, then escaped once for display."""

from __future__ import annotations

import base64
import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli, native_values
from eml_attachment_remover.domain import PathValue

if TYPE_CHECKING:
    from pathlib import Path


def _b64(raw: bytes) -> str:
    return base64.b64encode(raw).decode()


def _posix(parent: bytes | None, name: bytes) -> dict[str, object]:
    folder: dict[str, object] | None = None
    if parent is not None:
        folder = {
            "text": parent.decode(),
            "display": "ignored display",
            "native_base64": _b64(parent),
            "native_utf16le_base64": None,
        }
    return {"parent": folder, "basename_base64": _b64(name)}


def _windows(parent: str, name: str) -> dict[str, object]:
    return {
        "parent": {
            "text": parent,
            "display": "ignored display",
            "native_base64": None,
            "native_utf16le_base64": _b64(parent.encode("utf-16-le")),
        },
        "basename_base64": None,
        "basename_utf16le_base64": _b64(name.encode("utf-16-le")),
    }


@pytest.mark.parametrize(
    ("destination", "expected"),
    [
        (_posix(b"/out", b"a\nb.eml"), "/out/a\\u000ab.eml"),
        (_posix(b"/o\nut", b"a.eml"), "/o\\u000aut/a.eml"),
        (_posix(b"/out\r\n", b"a.eml"), "/out\\u000d\\u000a/a.eml"),
        (_posix(b"/out", b"a\xe2\x80\xaeb.eml"), "/out/a\\u202eb.eml"),
        (_posix(b"/out", b"a\xe2\x80\xa8b.eml"), "/out/a\\u2028b.eml"),
        # Only a slash ends a POSIX folder; a trailing backslash is a name character.
        (_posix(b"/out\\", b"a.eml"), "/out\\/a.eml"),
        (_posix(b"/out/", b"a.eml"), "/out/a.eml"),
        (_posix(b"/", b"a.eml"), "/a.eml"),
        (_windows("C:\\out", "a.eml"), "C:\\out\\a.eml"),
        (_windows("C:\\out\\", "a.eml"), "C:\\out\\a.eml"),
        (_windows("C:/out/", "a.eml"), "C:/out/a.eml"),
        (_windows("C:\\out", "a\nb.eml"), "C:\\out\\a\\u000ab.eml"),
    ],
)
def test_the_whole_planned_path_is_one_display_safe_line(
    destination: dict[str, object], expected: str
) -> None:
    """No control, bidi, or separator character survives, whatever its source."""
    display = native_values.planned_display(destination)
    assert display == expected
    assert display.isprintable()


def test_a_parent_without_evidence_uses_its_already_escaped_display() -> None:
    """A record with only a display keeps it; escaping twice changes nothing."""
    destination = _posix(None, b"a.eml")
    assert native_values.planned_display(destination) == "/a.eml"
    destination["parent"] = {"display": "/plan\\u000a"}
    assert native_values.planned_display(destination) == "/plan\\u000a/a.eml"


def test_native_evidence_wins_over_lossy_text_and_display() -> None:
    """The exact native bytes, not the escaped display, are what get joined."""
    destination = _posix(b"/real", b"a.eml")
    parent = destination["parent"]
    assert isinstance(parent, dict)
    parent["text"] = "/lossy"
    assert native_values.planned_display(destination) == "/real/a.eml"


def test_windows_addresses_are_measured_in_utf16_units() -> None:
    """A Windows receipt's address bound counts code units, not bytes."""
    wide = PathValue(None, "d", None, _b64("abc\U0001f600".encode("utf-16-le")))
    assert native_values.address_units(wide) == 5
    posix = PathValue("abc", "d", _b64(b"abc"))
    assert native_values.address_units(posix) == 3


@pytest.mark.skipif(os.name == "nt", reason="Windows names cannot contain newlines")
def test_a_dry_run_with_a_newline_name_prints_one_line(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The reported audit case: one would_create item is one physical line."""
    source = tmp_path / "a\nb.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert cli.main(["--dry-run", "--output-format=human", "--", str(source)]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 1
    assert lines[0].startswith("would_create: ")
    assert lines[0].endswith("a\\u000ab.mime-pruned.eml")
    assert "\\u000a" in lines[0].split(" -> ")[0]


@pytest.mark.skipif(os.name == "nt", reason="a backslash separates Windows folders")
def test_a_dry_run_in_a_backslash_named_directory_keeps_the_separator(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A POSIX directory whose name ends in a backslash is an ordinary directory."""
    folder = tmp_path / "out\\"
    folder.mkdir()
    source = folder / "a.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    assert cli.main(["--dry-run", "--output-format=human", "--", str(source)]) == 0
    target = capsys.readouterr().out.split(" -> ")[1].strip()
    assert target.endswith("out\\/a.mime-pruned.eml")

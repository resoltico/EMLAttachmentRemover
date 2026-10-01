"""Physical RFC 5322 names preserve spans without relaxing structured MIME values."""

from __future__ import annotations

from email import policy
from email.parser import BytesParser
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import mime_headers, process_file
from eml_attachment_remover.domain import ItemStatus
from tests.live_report_support import MESSAGE

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    "name",
    [
        b"X-Trace-Id",
        b"X-Trace/Id",
        b"X(Trace)",
        b"X?Trace",
        b"X,Trace",
        b"X[Trace]",
        b"X=Trace",
    ],
)
@pytest.mark.parametrize("first", [False, True])
def test_optional_physical_names_are_preserved_in_first_and_later_positions(
    tmp_path: Path,
    name: bytes,
    *,
    first: bool,
) -> None:
    field = name + b": public optional value\r\n"
    original = (
        field + MESSAGE
        if first
        else MESSAGE.replace(b"Subject:", field + b"Subject:", 1)
    )
    source = tmp_path / "source.eml"
    destination = tmp_path / "copy.eml"
    source.write_bytes(original)
    ledger = process_file(str(source), str(destination))
    assert ledger.items[0].status is ItemStatus.CREATED
    assert source.read_bytes() == original
    output = destination.read_bytes()
    assert field in output
    assert output.count(field) == 1
    parsed = BytesParser(policy=policy.default).parsebytes(output)
    assert parsed[name.decode("ascii")] == "public optional value"
    assert not parsed.defects
    assert parsed.get_body().get_content().strip() == "public body"  # type: ignore[union-attr]
    assert not list(parsed.iter_attachments())


@pytest.mark.parametrize(
    "name",
    [
        b"",
        b"X Trace",
        b"X:Trace",
        b"X\tTrace",
        b"X\x00",
        b"X\x1f",
        b"X\x7f",
        b"X\xff",
        b"X\r\nTrace",
    ],
)
def test_invalid_names_remain_outside_the_physical_grammar(name: bytes) -> None:
    assert not mime_headers.is_header_name(name)


def test_physical_recognition_accepts_exact_ascii_boundaries() -> None:
    for byte in range(256):
        assert mime_headers.is_header_name(bytes([byte])) is (
            33 <= byte <= 126 and byte != 58
        )


@pytest.mark.parametrize(
    "control",
    [
        b"Content-Type: text/(plain)",
        b"Content-Transfer-Encoding: 7?bit",
        b"Content-Disposition: attach/ment",
        b"Content-Type: text/plain\r\nContent-Type: text/html",
    ],
)
def test_malformed_mime_controls_still_refuse_publication(
    tmp_path: Path,
    control: bytes,
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(b"X(Trace): preserved\r\n" + control + b"\r\n\r\nbody")
    ledger = process_file(str(source))
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not list(tmp_path.glob("*.mime-pruned.eml"))

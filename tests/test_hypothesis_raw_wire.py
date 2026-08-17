"""Property-test bounded noncanonical raw MIME wire representations."""

from __future__ import annotations

import base64
import quopri
import string
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Final

import pytest
from hypothesis import event, given, target
from hypothesis import strategies as st

from eml_attachment_remover import process_file
from eml_attachment_remover.models import CliError, ExitCode
from tests.test_support import decoded_hash, parse

TOKEN: Final[st.SearchStrategy[str]] = st.text(
    alphabet=string.ascii_letters + string.digits,
    min_size=1,
    max_size=24,
)
BODY: Final[st.SearchStrategy[str]] = st.text(
    alphabet=string.ascii_letters + string.digits + " .,;:!?žā漢字",
    max_size=160,
)
PAYLOAD: Final[st.SearchStrategy[bytes]] = st.integers(
    min_value=0,
    max_value=512,
).flatmap(lambda size: st.binary(min_size=size, max_size=size))


@dataclass(frozen=True, slots=True)
class WireCase:
    """Describe independent representation choices for one raw MIME message."""

    token: str
    body: str
    payload: bytes
    newline: str
    folded_disposition: bool
    transfer_encoding: str
    preamble: bool
    epilogue: bool

    @property
    def boundary(self) -> str:
        """A boundary guaranteed not to collide with encoded test payloads."""
        return f"=_public-boundary-{self.token}"


def _wire_cases(epilogue: st.SearchStrategy[bool]) -> st.SearchStrategy[WireCase]:
    """Build raw-wire cases with an explicitly meaningful epilogue axis.

    Returns:
        The configured raw-wire strategy.

    """
    return st.builds(
        WireCase,
        token=TOKEN,
        body=BODY,
        payload=PAYLOAD,
        newline=st.sampled_from(("\n", "\r\n")),
        folded_disposition=st.booleans(),
        transfer_encoding=st.sampled_from(("base64", "quoted-printable")),
        preamble=st.booleans(),
        epilogue=epilogue,
    )


WIRE_CASES: Final = _wire_cases(st.booleans())
MALFORMED_WIRE_CASES: Final = _wire_cases(st.just(value=False))


def _encode_payload(case: WireCase) -> str:
    """Encode a generated binary payload for its declared transfer encoding.

    Returns:
        ASCII transfer-encoded payload text.

    """
    if case.transfer_encoding == "base64":
        return base64.b64encode(case.payload).decode("ascii")
    return quopri.encodestring(case.payload, quotetabs=True).decode("ascii")


def _wire_lines(case: WireCase, *, close_boundary: bool) -> list[str]:
    """Build noncanonical but bounded MIME wire lines.

    Returns:
        Header, body, boundary, and optional preamble/epilogue lines.

    """
    disposition = (
        ["Content-Disposition: attachment;", '\tfilename="public-file.bin"']
        if case.folded_disposition
        else ['Content-Disposition: attachment; filename="public-file.bin"']
    )
    lines = [
        "From: sender@example.test",
        "To: recipient@example.test",
        "Subject: Raw public MIME fixture",
        "MIME-Version: 1.0",
        f'Content-Type: multipart/mixed; boundary="{case.boundary}"',
        "",
    ]
    if case.preamble:
        lines.append("Public MIME preamble")
    lines.extend((
        f"--{case.boundary}",
        'Content-Type: text/plain; charset="utf-8"',
        "Content-Transfer-Encoding: base64",
        "",
        base64.b64encode(case.body.encode()).decode("ascii"),
        f"--{case.boundary}",
        "Content-Type: application/octet-stream",
        f"Content-Transfer-Encoding: {case.transfer_encoding}",
        *disposition,
        "",
        _encode_payload(case),
    ))
    if close_boundary:
        lines.append(f"--{case.boundary}--")
        if case.epilogue:
            lines.append("Public MIME epilogue")
    return lines


def _wire_bytes(case: WireCase, *, close_boundary: bool = True) -> bytes:
    """Serialize generated wire lines with the selected newline convention.

    Returns:
        Raw MIME message bytes.

    """
    return (
        case.newline.join(_wire_lines(case, close_boundary=close_boundary))
        + case.newline
    ).encode("ascii")


@given(case=WIRE_CASES)
def test_noncanonical_wire_message_removes_only_the_attachment(
    case: WireCase,
) -> None:
    """Check accepted noncanonical MIME representations end to end."""
    raw = _wire_bytes(case)
    event(f"wire-newline={'crlf' if case.newline == chr(13) + chr(10) else 'lf'}")
    event(f"wire-transfer-encoding={case.transfer_encoding}")
    event(f"wire-folded-disposition={case.folded_disposition}")
    target(len(case.payload), label="raw attachment payload size")
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "raw.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)
        source_message = parse(source)
        result = process_file(source, destination, force=False, dry_run=False)
        output = parse(destination)
        body = next(
            part for part in output.walk() if part.get_content_type() == "text/plain"
        )
        source_body = next(
            part
            for part in source_message.walk()
            if part.get_content_type() == "text/plain"
        )
        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed_attachments] == [
            "public-file.bin"
        ]
        assert decoded_hash(body) == decoded_hash(source_body)
        assert body is output
        assert output.preamble is None
        assert output.epilogue is None


@given(case=MALFORMED_WIRE_CASES)
def test_missing_closing_boundary_is_rejected_without_output(
    case: WireCase,
) -> None:
    """Reject malformed MIME representations without writing output."""
    raw = _wire_bytes(case, close_boundary=False)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "malformed.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)
        with pytest.raises(CliError) as raised:
            process_file(source, destination, force=False, dry_run=False)
        assert raised.value.code == ExitCode.PARSE_ERROR
        assert source.read_bytes() == raw
        assert not destination.exists()

"""Property tests for attachment removal and Content-ID retention."""

from __future__ import annotations

import hashlib
import string
import tempfile
from collections import Counter
from email import policy
from email.message import EmailMessage
from operator import add
from pathlib import Path
from typing import Final

from hypothesis import event, example, given, target
from hypothesis import strategies as st

from eml_attachment_remover import process_file
from eml_attachment_remover.models import KeepReason
from tests.test_support import decoded_hash, leaf_rows, parse

type MediaSpec = tuple[str, str, str]

FILENAME_CHARACTERS: Final = (
    string.ascii_letters + string.digits + " ._-()'\";=%&[]{}!#žā漢字🚚"
)
FILENAME_EDGE_CHARACTERS: Final = string.ascii_letters + string.digits + "žā漢字🚚"
IDENTIFIER_CHARACTERS: Final = string.ascii_letters + string.digits + "._+-"
MEDIA_SPECS: Final[tuple[MediaSpec, ...]] = (
    ("application", "octet-stream", "bin"),
    ("application", "pdf", "pdf"),
    ("image", "jpeg", "jpg"),
    ("text", "csv", "csv"),
)
FILENAME: Final[st.SearchStrategy[str]] = st.builds(
    add,
    st.builds(
        add,
        st.sampled_from(FILENAME_EDGE_CHARACTERS),
        st.text(alphabet=FILENAME_CHARACTERS, max_size=62),
    ),
    st.sampled_from(FILENAME_EDGE_CHARACTERS),
)
BODY_TEXT: Final[st.SearchStrategy[str]] = st.text(
    alphabet=st.characters(blacklist_categories=("Cc", "Cs")) | st.sampled_from("\n\t"),
    max_size=256,
)
IDENTIFIER_BASE: Final[st.SearchStrategy[str]] = st.builds(
    add,
    st.sampled_from(string.ascii_letters + string.digits),
    st.text(alphabet=IDENTIFIER_CHARACTERS, max_size=36),
)
PAYLOAD: Final[st.SearchStrategy[bytes]] = st.integers(
    min_value=0,
    max_value=1_024,
).flatmap(lambda size: st.binary(min_size=size, max_size=size))


def _attachment_part(
    payload: bytes,
    *,
    filename: str,
    content_id: str,
) -> EmailMessage:
    """Create a binary attachment with an explicitly represented Content-ID.

    Returns:
        The configured MIME entity.

    """
    part = EmailMessage()
    part.set_content(
        payload,
        maintype="image",
        subtype="png",
        cte="base64",
    )
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-ID"] = content_id
    return part


def _header_identifier(identifier: str, form: str) -> str:
    """Represent one semantic identifier as a Content-ID header value.

    Returns:
        The represented header value.

    """
    if form == "bare":
        return identifier
    if form == "spaced":
        return f" <{identifier}> "
    return f"<{identifier}>"


def _body_identifier(identifier: str, form: str) -> str:
    """Represent one semantic identifier as a body CID URI.

    Returns:
        The represented body URI.

    """
    if form == "angle":
        return f"cid:<{identifier}>"
    if form == "encoded":
        return f"cid:%3C{identifier}%3E"
    if form == "entity":
        return f"cid:&lt;{identifier}&gt;"
    if form == "spaced":
        return f"CID: <{identifier}>"
    return f"cid:{identifier}"


@example(
    filename="report (final).bin",
    payload=b"public payload",
    body="public body",
    media=("application", "octet-stream", "bin"),
)
@example(
    filename="žā漢字🚚.bin",
    payload=b"\x00\xff",
    body="Unicode body žā漢字🚚",
    media=("image", "jpeg", "jpg"),
)
@given(
    filename=FILENAME,
    payload=PAYLOAD,
    body=BODY_TEXT,
    media=st.sampled_from(MEDIA_SPECS),
)
def test_generated_attachment_removal_matches_exact_output_model(
    filename: str,
    payload: bytes,
    body: str,
    media: MediaSpec,
) -> None:
    """Check exact end-to-end attachment removal over generated public data."""
    maintype, subtype, _extension = media
    message = EmailMessage()
    message["Subject"] = "Generated public attachment fixture"
    message["X-Public-Invariant"] = "retained"
    message.set_content(body)
    message.add_attachment(
        payload,
        maintype=maintype,
        subtype=subtype,
        filename=filename,
    )
    raw = message.as_bytes(policy=policy.SMTP)
    event(f"attachment-media={maintype}/{subtype}")
    event(f"filename-has-unicode={not filename.isascii()}")
    event(f"payload-size-bucket={min(len(payload) // 128, 8)}")
    target(len(filename), label="attachment filename length")
    target(len(payload), label="attachment payload size")
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)
        parsed_source = parse(source)
        expected_rows = Counter(
            row for row in leaf_rows(parsed_source) if row[2] != filename
        )
        result = process_file(source, destination, force=False, dry_run=False)
        parsed_output = parse(destination)
        assert source.read_bytes() == raw
        assert len(result.removed) == 1
        assert result.removed[0].filename == filename
        assert result.removed[0].content_type == f"{maintype}/{subtype}"
        assert result.removed[0].disposition == "attachment"
        assert Counter(leaf_rows(parsed_output)) == expected_rows
        assert parsed_output["X-Public-Invariant"] == "retained"


@example(
    base="inline.resource+public",
    payload=b"\x00" * 1_024,
    header_form="angle",
    body_form="encoded",
)
@given(
    base=IDENTIFIER_BASE,
    payload=PAYLOAD,
    header_form=st.sampled_from(("angle", "bare", "spaced")),
    body_form=st.sampled_from(("plain", "angle", "encoded", "entity", "spaced")),
)
def test_cid_representations_retain_only_the_exact_resource(
    base: str,
    payload: bytes,
    header_form: str,
    body_form: str,
) -> None:
    """Check end-to-end CID normalization and exact resource matching."""
    identifier = f"{base}-target@example.test"
    near_identifier = f"{base}-targetx@example.test"
    message = EmailMessage()
    message["Subject"] = "Generated public CID-reference fixture"
    message.set_content(
        '<html><body data-public="retained"><img src="'
        f"{_body_identifier(identifier, body_form)}"
        '"></body></html>',
        subtype="html",
    )
    message.make_mixed()
    message.attach(
        _attachment_part(
            payload,
            filename="referenced.png",
            content_id=_header_identifier(identifier, header_form),
        )
    )
    message.attach(
        _attachment_part(
            b"public near-match attachment",
            filename="near-match.png",
            content_id=f"<{near_identifier}>",
        )
    )
    raw = message.as_bytes(policy=policy.SMTP)
    event(f"cid-header-form={header_form}")
    event(f"cid-body-form={body_form}")
    target(len(identifier), label="Content-ID length")
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)
        expected_rows = Counter(
            row for row in leaf_rows(parse(source)) if row[2] != "near-match.png"
        )
        result = process_file(source, destination, force=False, dry_run=False)
        parsed_output = parse(destination)
        retained = next(
            part
            for part in parsed_output.walk()
            if part.get_filename() == "referenced.png"
        )
        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed] == ["near-match.png"]
        assert [part.reason for part in result.preserved_file_parts] == [
            KeepReason.BODY_CID_REFERENCE
        ]
        assert Counter(leaf_rows(parsed_output)) == expected_rows
        assert decoded_hash(retained) == hashlib.sha256(payload).hexdigest()

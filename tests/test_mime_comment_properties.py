"""Generated comment grammar preserves field semantics and rejects truncation."""

from __future__ import annotations

import string

import pytest
from hypothesis import given
from hypothesis import strategies as st

from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_raw import parse_raw_mime


@given(
    st.text(
        alphabet=string.ascii_letters + string.digits + "\\()", min_size=1, max_size=12
    ).map(
        lambda value: (
            value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        )
    )
)
def test_property_structured_comments_preserve_content_type_semantics(  # type: ignore[misc]
    label: str,
) -> None:
    """Permitted trailing comments do not change a synthetic Content-Type token."""
    raw = (
        b"Content-Type: text/plain; charset=us-ascii ("
        + label.encode("ascii")
        + b")\r\n\r\nbody\r\n"
    )
    assert parse_raw_mime(raw).root.media_type == "text/plain"


@given(
    st.text(
        alphabet=string.ascii_letters + string.digits + "\\()", min_size=1, max_size=8
    ).map(
        lambda value: (
            value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        )
    )
)
def test_property_structured_grammar_keeps_comments_and_rejects_splices(  # type: ignore[misc]
    label: str,
) -> None:
    """Keep supported CFWS semantic-free and reject malformed token splices."""
    comment = label.encode("ascii")
    valid = (
        b"Content-Type: text/plain; charset=us-ascii (outer ("
        + comment
        + b"))\r\nContent-Transfer-Encoding: 7bit (ASCII)\r\n\r\nbody\r\n"
    )
    assert parse_raw_mime(valid).root.media_type == "text/plain"
    malformed = b"Content-Type: text/plain; charset=us-ascii (" + comment
    with pytest.raises(AppError):
        parse_raw_mime(malformed + b"\r\n\r\nbody\r\n")

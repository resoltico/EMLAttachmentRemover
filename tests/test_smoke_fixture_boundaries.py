# ruff: file-ignore[private-member-access]
"""Close defensive standard-library builder boundaries in smoke fixtures."""

from __future__ import annotations

from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from unittest.mock import patch

import pytest
from tools import smoke_distribution


def test_fixture_contains_the_documented_public_semantics() -> None:
    """Pin every field that makes the installed smoke fixture meaningful."""
    content = smoke_distribution._fixture()
    assert b"Subject: Installed distribution smoke test\r\n" in content
    assert b"Content-Type: application/octet-stream\r\n" in content
    assert b"\r\n" in content
    assert b"\n\n" not in content
    parsed = BytesParser(policy=policy.default).parsebytes(content)
    attachments = [
        part for part in parsed.walk() if part.get_content_disposition() == "attachment"
    ]
    resources = [
        part for part in parsed.walk() if part.get_content_disposition() == "inline"
    ]
    assert len(attachments) == 1
    assert attachments[0].get_filename() == "public.bin"
    assert (
        attachments[0].get_payload(decode=True) == smoke_distribution.ATTACHMENT_PAYLOAD
    )
    assert len(resources) == 1
    assert str(resources[0]["Content-Type"]) == (
        f"{smoke_distribution.BODY_RESOURCE_MAIN_TYPE}/"
        f"{smoke_distribution.BODY_RESOURCE_SUBTYPE}"
    )
    assert (
        resources[0][smoke_distribution.CONTENT_ID_HEADER]
        == smoke_distribution.BODY_RESOURCE_CONTENT_ID
    )
    assert resources[0].get_filename() == smoke_distribution.BODY_RESOURCE_FILENAME
    assert (
        resources[0].get_content_disposition()
        == smoke_distribution.BODY_RESOURCE_DISPOSITION
    )
    assert resources[0].get_payload(decode=True) == smoke_distribution.BODY_RESOURCE
    assert (
        f"cid:{smoke_distribution.BODY_RESOURCE_CONTENT_ID[1:-1]}".encode() in content
    )


def test_fixture_rejects_a_builder_that_does_not_make_an_alternative() -> None:
    """Reject a builder result whose payload never becomes multipart."""
    with patch.object(EmailMessage, "add_alternative", return_value=None):
        with pytest.raises(TypeError) as raised:
            smoke_distribution._fixture()

    assert str(raised.value) == "synthetic alternative payload was not multipart"


def test_fixture_rejects_a_non_message_html_alternative() -> None:
    """Reject a multipart builder result with a non-message HTML child."""

    def invalid_alternative(
        message: EmailMessage,
        _content: str,
        *,
        subtype: str,
    ) -> None:
        assert subtype == "html"
        message.set_payload([EmailMessage(), "PUBLIC INVALID HTML ENTITY"])

    with patch.object(
        EmailMessage,
        "add_alternative",
        new=invalid_alternative,
    ):
        with pytest.raises(TypeError) as raised:
            smoke_distribution._fixture()

    assert str(raised.value) == "synthetic HTML alternative was not an email entity"

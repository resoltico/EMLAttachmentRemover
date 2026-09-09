"""Synthetic fail-closed decision-table contracts for the v3 MIME policy."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_policy
from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_validation import ContentSpec


@pytest.mark.parametrize(
    "raw",
    [
        b"Content-Type: text/plain\r\nContent-Disposition: attachment\r\n\r\nbody\r\n",
        b"Content-Type: text/plain\r\nContent-Disposition: form-data\r\n\r\nbody\r\n",
        b"Content-Type: text/plain; name=body.txt\r\n\r\nbody\r\n",
        b"Content-Type: application/pgp-signature\r\n\r\nbody\r\n",
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n--a--\r\n",
        (
            b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
            b"--a\r\nContent-Type: image/png\r\n\r\nx\r\n--a--\r\n"
        ),
        (
            b"Content-Type: multipart/alternative; boundary=a\r\n"
            b"Content-Disposition: attachment\r\n\r\n--a\r\n"
            b"Content-Type: text/plain\r\n\r\nx\r\n--a--\r\n"
        ),
        (
            b"Content-Type: multipart/related; boundary=r\r\n"
            b"Content-Disposition: attachment\r\n\r\n--r\r\n"
            b"Content-Type: text/plain\r\n\r\nx\r\n--r--\r\n"
        ),
        b"Content-Type: multipart/related; boundary=r\r\n\r\n--r--\r\n",
        (
            b"Content-Type: multipart/mixed; boundary=m\r\n"
            b"Content-Disposition: attachment\r\n\r\n--m\r\n"
            b"Content-Type: text/plain\r\n\r\nx\r\n--m--\r\n"
        ),
    ],
)
def test_policy_rejects_ambiguous_or_conflicting_roles(raw: bytes) -> None:
    with pytest.raises(AppError):
        classify(parse_raw_mime(raw).root)


def test_related_policy_rejects_duplicate_cid_type_and_start_info_targets() -> None:
    duplicate = (
        b"Content-Type: multipart/related; boundary=r\r\n\r\n"
        b"--r\r\nContent-Type: text/plain\r\nContent-ID: <same@x>\r\n\r\nx\r\n"
        b"--r\r\nContent-Type: text/html\r\nContent-ID: <same@x>\r\n\r\nx\r\n--r--\r\n"
    )
    wrong_type = (
        b'Content-Type: multipart/related; boundary=r; type="text/html"\r\n\r\n'
        b"--r\r\nContent-Type: text/plain\r\n\r\nx\r\n--r--\r\n"
    )
    removed_target = (
        b'Content-Type: multipart/related; boundary=r; start-info="<gone@x>"\r\n\r\n'
        b"--r\r\nContent-Type: text/plain\r\n\r\nx\r\n"
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <gone@x>\r\n\r\nx\r\n--r--\r\n"
    )
    for raw in (duplicate, wrong_type, removed_target):
        with pytest.raises(AppError):
            classify(parse_raw_mime(raw).root)


def test_policy_directly_rejects_empty_and_missing_related_roots() -> None:
    alternative = RawNode(
        (), 0, 0, 0, (), ContentSpec("multipart/alternative", {}), None, "7bit"
    )
    related = RawNode(
        (), 0, 0, 0, (), ContentSpec("multipart/related", {}), None, "7bit"
    )
    child = RawNode((0,), 0, 0, 0, (), ContentSpec("text/plain", {}), None, "7bit")
    missing = RawNode(
        (),
        0,
        0,
        0,
        (),
        ContentSpec("multipart/related", {b"start": b"<missing@x>"}),
        None,
        "7bit",
        [child],
    )
    for root in (alternative, related, missing):
        with pytest.raises(AppError):
            classify(root)


def test_related_start_info_distinguishes_empty_literal_and_duplicate() -> None:
    assert mime_policy._start_info_ids(b" \t") is None  # ruff: ignore[private-member-access] - direct start-info grammar contract.
    assert mime_policy._start_info_ids(b"literal") is None  # ruff: ignore[private-member-access] - direct start-info grammar contract.
    with pytest.raises(AppError):
        mime_policy._start_info_ids(b"<a@x> <a@x>")  # ruff: ignore[private-member-access] - direct start-info grammar contract.

"""Exact transfer-decoding and independent-stdlib validation receipts."""

from __future__ import annotations

from email import errors
from email.message import EmailMessage
from types import SimpleNamespace
from typing import cast

import pytest

from eml_attachment_remover import mime_encoding, mime_stdlib_check
from eml_attachment_remover.domain import AppError, ExitCode, RetainedFingerprint
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_stdlib_check import StdlibValidationWork
from eml_attachment_remover.mime_validation import ContentSpec


def _node(*, headers: tuple[Header, ...] = (), opaque: bool = False) -> RawNode:
    """Make one leaf whose raw ownership facts are explicit.

    Returns:
        A 7bit text/plain leaf whose supplied facts are the only varying controls.

    """
    return RawNode(
        (), 0, 0, 0, headers, ContentSpec("text/plain", {}), None, "7bit", opaque=opaque
    )


@pytest.mark.parametrize(
    ("encoded", "cte", "expected"),
    [
        (b"\x00\x7f", "7bit", b"\x00\x7f"),
        (b"YQ== \t\r\n", "base64", b"a"),
        (b"one=3Dtwo=\r\nthree=\nfour", "quoted-printable", b"one=twothreefour"),
    ],
)
def test_transfer_decoder_has_exact_valid_cte_and_transport_whitespace_contract(
    encoded: bytes, cte: str, expected: bytes
) -> None:
    """Each retained encoding accepts only its defined source form and octets."""
    assert mime_encoding.decode_payload(encoded, cte) == expected


@pytest.mark.parametrize(
    ("encoded", "cte", "message"),
    [
        (b"\x80", "7bit", "7bit payload contains an 8bit octet"),
        (b"YQ==\v", "base64", "invalid retained base64 payload"),
        (b"YQ==\f", "base64", "invalid retained base64 payload"),
        (b"YQ=", "base64", "invalid retained base64 payload"),
        (b"=\r", "quoted-printable", "invalid retained quoted-printable payload"),
        (b"=4", "quoted-printable", "invalid retained quoted-printable payload"),
        (b"=4G", "quoted-printable", "invalid retained quoted-printable payload"),
        (b"=G4", "quoted-printable", "invalid retained quoted-printable payload"),
        (b"plain", "x-custom", "unsupported Content-Transfer-Encoding x-custom"),
    ],
)
def test_transfer_decoder_rejects_each_invalid_wire_form_exactly(
    encoded: bytes, cte: str, message: str
) -> None:
    """Invalid transfer encodings cannot be normalized into a retained body."""
    with pytest.raises(AppError) as raised:
        mime_encoding.decode_payload(encoded, cte)
    assert raised.value == AppError(ExitCode.PARSE_ERROR, message)


def test_fingerprints_exclude_containers_and_bind_sorted_source_facts() -> None:
    """Every retained leaf carries stable source, decoded, and metadata evidence."""
    raw = (
        b"Content-Type: multipart/mixed; z=last; a=first; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain; z=last; a=first\r\n"
        b"Content-Transfer-Encoding: base64\r\n\r\nYWxwaGE=\r\n"
        b"--m\r\nContent-Type: text/html; charset=utf-8\r\n"
        b"Content-Transfer-Encoding: quoted-printable\r\n\r\nbe=74a=3D\r\n"
        b"--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    assert mime_encoding.fingerprint_retained(raw, list(tree.nodes)) == (
        RetainedFingerprint(
            (0,),
            "text/plain",
            "base64",
            ((b"a", b"first"), (b"z", b"last")),
            "f631784c0c2f52ce25f9832d84401d587a70756864d8d139788db656913945fc",
            "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8",
        ),
        RetainedFingerprint(
            (1,),
            "text/html",
            "quoted-printable",
            ((b"charset", b"utf-8"),),
            "47c21fef02a891d1d38f7d29a545d288e066e340054ff41de0fca6013b7a2b95",
            "8c4908a188605bc8dcd640fd2743b0e02197fb6bd01205edc7b112119628b045",
        ),
    )


def test_fingerprints_enforce_the_inclusive_cumulative_decoded_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A budget equal to one leaf passes, while the next decoded leaf is rejected."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\na\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nb\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    with monkeypatch.context() as context:
        context.setattr(mime_encoding, "MAX_RETAINED_DECODED", 1)
        with pytest.raises(AppError) as raised:
            mime_encoding.fingerprint_retained(raw, list(tree.nodes))
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "retained decoded payload limit exceeded"
    )


def test_stdlib_defects_allow_only_headerless_separator_diagnostics() -> None:
    """Only opaque roots and one headerless defect have scoped exemptions."""
    headerless = _node()
    separator_defect = errors.MissingHeaderBodySeparatorDefect()
    mime_stdlib_check._validate_defects(  # ruff: ignore[private-member-access] - exact stdlib defect exemption.
        cast("EmailMessage", SimpleNamespace(defects=(separator_defect,))), headerless
    )
    malformed = errors.FirstHeaderLineIsContinuationDefect("continued")
    for node in (
        _node(headers=(Header(b"content-type", b"text/plain", 0, 26),)),
        _node(),
    ):
        defect = separator_defect if node.headers else malformed
        with pytest.raises(AppError) as raised:
            mime_stdlib_check._validate_defects(  # ruff: ignore[private-member-access] - exact stdlib defect rejection.
                cast("EmailMessage", SimpleNamespace(defects=(defect,))), node
            )
        assert raised.value == AppError(
            ExitCode.PARSE_ERROR, "stdlib MIME parser reported a defect"
        )
    mime_stdlib_check._validate_defects(  # ruff: ignore[private-member-access] - opaque removal exemption.
        cast("EmailMessage", SimpleNamespace(defects=(malformed,))), _node(opaque=True)
    )


def test_stdlib_children_and_headers_require_complete_message_evidence() -> None:
    """Child edges and every physical header participate in independent validation."""
    container = EmailMessage()
    first = EmailMessage()
    second = EmailMessage()
    container.set_payload([first, second])
    assert mime_stdlib_check._children(  # ruff: ignore[private-member-access] - exact child-edge receipt.
        container
    ) == [first, second]
    assert (
        mime_stdlib_check._children(  # ruff: ignore[private-member-access] - leaf child-edge receipt.
            EmailMessage()
        )
        == []
    )
    valid_headers = cast(
        "EmailMessage",
        {
            "Content-Type": SimpleNamespace(defects=()),
            "X-Trace": SimpleNamespace(defects=()),
        },
    )
    mime_stdlib_check._validate_headers(  # ruff: ignore[private-member-access] - every parsed header must be clean.
        valid_headers
    )
    defective_headers = cast(
        "EmailMessage",
        {
            "Content-Type": SimpleNamespace(defects=()),
            "X-Trace": SimpleNamespace(defects=(ValueError(),)),
        },
    )
    with pytest.raises(AppError) as raised:
        mime_stdlib_check._validate_headers(  # ruff: ignore[private-member-access] - later header defects cannot be skipped.
            defective_headers
        )
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "MIME header parser reported a defect"
    )


def test_stdlib_ownership_compares_order_case_and_declared_media_type() -> None:
    """Raw physical-header ownership is ordered, case-normalized, and type-bound."""
    node = _node(
        headers=(
            Header(b"content-type", b"text/plain", 0, 26),
            Header(b"x-trace", b"one", 26, 40),
        )
    )
    matching = cast(
        "EmailMessage",
        SimpleNamespace(
            raw_items=lambda: (("Content-Type", "text/plain"), ("X-Trace", "two")),
            get_content_type=lambda: "TEXT/PLAIN",
        ),
    )
    mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - exact independent ownership receipt.
        matching, node
    )
    invalid = (
        SimpleNamespace(
            raw_items=lambda: (("X-Trace", "two"), ("Content-Type", "text/plain")),
            get_content_type=lambda: "text/plain",
        ),
        SimpleNamespace(
            raw_items=lambda: (("Content-Type", "text/plain"), ("X-Trace", "two")),
            get_content_type=lambda: "text/html",
        ),
    )
    messages = (
        "raw MIME header ownership disagrees with stdlib",
        "raw MIME media type disagrees with stdlib",
    )
    for part, message in zip(invalid, messages, strict=True):
        with pytest.raises(AppError) as raised:
            mime_stdlib_check._compare_ownership(  # ruff: ignore[private-member-access] - rejected ownership/type mismatch.
                cast("EmailMessage", part), node
            )
        assert raised.value == AppError(ExitCode.PARSE_ERROR, message)


def test_stdlib_traversal_reports_all_direct_work_but_not_opaque_descendants() -> None:
    """The cross-check counts each indexed node and only traversed child edge once."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: multipart/mixed; boundary=n\r\n"
        b"Content-Disposition: attachment\r\n\r\n"
        b"--n\r\nContent-Type: text/plain\r\n\r\nopaque\r\n--n--\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    assert tree.stdlib_work == StdlibValidationWork(node_visits=3, child_edge_visits=2)
    assert mime_stdlib_check.validate_stdlib_tree(
        mime_stdlib_check.parse_stdlib(raw), tree.by_path
    ) == StdlibValidationWork(node_visits=3, child_edge_visits=2)

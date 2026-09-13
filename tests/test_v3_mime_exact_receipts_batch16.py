"""Exact MIME receipts for residual semantic mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import (
    mime_encoding,
    mime_headers,
    mime_policy,
    mime_raw,
    mime_validation,
)
from eml_attachment_remover.domain import AppError, DecisionAction, ExitCode
from eml_attachment_remover.mime_raw import RawNode
from eml_attachment_remover.mime_validation import ContentSpec


def _node(
    path: tuple[int, ...] = (),
    media_type: str = "text/plain",
    *,
    parameters: dict[bytes, bytes] | None = None,
    disposition: ContentSpec | None = None,
) -> RawNode:
    """Build the smallest leaf whose MIME ownership facts are explicit.

    Returns:
        A leaf with no headers or children and only the supplied role facts.

    """
    return RawNode(
        path,
        0,
        0,
        0,
        (),
        ContentSpec(media_type, {} if parameters is None else parameters),
        disposition,
        "8bit",
    )


def test_transfer_decoding_preserves_lower_hex_and_encoded_x_octets() -> None:
    """Strict decoders accept legal lower-case hex and do not strip base64 X data."""
    assert mime_encoding.decode_payload(b"=ab=bC", "quoted-printable") == b"\xab\xbc"
    assert mime_encoding.decode_payload(b"XXU=", "base64") == b"]u"


def test_retained_decoded_budget_allows_exact_cumulative_boundary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A decoded total exactly at the public limit remains an admissible payload."""
    first = _node()
    first.end = 1
    second = _node((1,))
    second.start = 1
    second.end = 2
    second.body_start = 1
    with monkeypatch.context() as context:
        context.setattr(mime_encoding, "MAX_RETAINED_DECODED", 2)
        fingerprints = mime_encoding.fingerprint_retained(b"ab", [first, second])
    assert [fingerprint.source_path for fingerprint in fingerprints] == [(), (1,)]


def test_header_scanners_keep_zero_newlines_and_bounded_mbox_offsets() -> None:
    """Wire indices must accept a newline at offset zero and a nonzero entity start."""
    assert mime_headers.line_end(b"\nbody", 0, 5) == 1
    prefix = b"prefix"
    raw = prefix + b"From sender@example.test\r\nX: value\r\n"
    assert mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - exact bounded mbox-offset receipt.
        raw, len(prefix), len(raw)
    ) == len(prefix) + len(b"From sender@example.test\r\n")


def test_raw_header_probe_never_borrows_bytes_outside_its_entity() -> None:
    """A truncated entity is headerless even if a later source byte supplies a colon."""
    assert mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - CRLF physical-header probe.
        b"X:\r\n", 0, 4
    )
    assert mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - LF physical-header probe.
        b"X:\n", 0, 3
    )
    assert mime_raw._entity_headers(  # ruff: ignore[private-member-access] - entity boundary must be authoritative.
        b"X:\r\n\r\n", 0, 1
    ) == ((), 0, 0)


def test_root_node_budget_starts_at_zero_before_the_first_node(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """One root node is valid when the public total-node ceiling is one."""
    raw = b"Content-Type: text/plain\r\n\r\nbody\r\n"
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_NODES", 1)
        tree = mime_raw.parse_raw_mime(raw)
    assert [node.path for node in tree.nodes] == [()]


def test_related_raw_index_preserves_field_and_container_path_diagnostics() -> None:
    """Raw related parsing retains the field label and failing compound path."""
    malformed_start = _node(
        (4,), "multipart/related", parameters={b"start": b"<missing"}
    )
    with pytest.raises(AppError) as malformed:
        mime_raw._related_root_index(  # ruff: ignore[private-member-access] - raw related-start grammar receipt.
            malformed_start, []
        )
    assert malformed.value == AppError(
        ExitCode.PARSE_ERROR, "malformed multipart/related start"
    )

    missing_closing = _node((7,), "multipart/mixed", parameters={b"boundary": b"part"})
    missing_closing.end = len(b"--part\r\nbody\r\n")
    with pytest.raises(AppError) as closing:
        mime_raw._parse_multipart(  # ruff: ignore[private-member-access] - path-qualified delimiter failure receipt.
            b"--part\r\nbody\r\n", missing_closing, [0, 0]
        )
    assert closing.value == AppError(
        ExitCode.PARSE_ERROR, "multipart entity lacks a closing boundary", (7,)
    )


def test_policy_filename_and_identifier_contexts_remain_closed() -> None:
    """Filename metadata and malformed related IDs retain their exact role contracts."""
    named_inline = _node(disposition=ContentSpec("inline", {b"filename": b"body.txt"}))
    assert mime_policy._has_filename(  # ruff: ignore[private-member-access] - disposition filename role receipt.
        named_inline
    )
    assert mime_policy.classify(named_inline).actions == {(): DecisionAction.KEEP}

    for parser, message in (
        (mime_policy._parse_start, "malformed multipart/related start"),  # ruff: ignore[private-member-access] - related-start error context.
        (mime_policy._start_info_ids, "malformed related start-info"),  # ruff: ignore[private-member-access] - start-info error context.
    ):
        with pytest.raises(AppError) as malformed:
            parser(b"<missing")
        assert malformed.value == AppError(ExitCode.PARSE_ERROR, message)


def test_policy_requires_a_kept_supported_leaf_not_merely_a_kept_leaf() -> None:
    """An internal action map cannot claim an unsupported leaf as a retained body."""
    classifier = mime_policy._Classifier()  # ruff: ignore[private-member-access] - retained-leaf invariant receipt.
    unsupported = _node(media_type="application/json")
    classifier.actions[()] = DecisionAction.KEEP
    assert not classifier._has_retained_leaf(  # ruff: ignore[private-member-access] - policy helper must honor supported media types.
        unsupported
    )


def test_rfc2231_parameter_grammar_keeps_its_wire_boundaries() -> None:
    """A first equals, encoded flag, and hexadecimal alphabet each remain observable."""
    assert mime_validation._parameter_piece(  # ruff: ignore[private-member-access] - quoted equals stays inside the value.
        b'filename="one=two"'
    ) == (b"filename", None, b"one=two")
    assert mime_validation._parameter_piece(  # ruff: ignore[private-member-access] - RFC 2231 encoded parameter receipt.
        b"filename*=utf-8''%41"
    ) == (b"filename", None, b"utf-8''%41")
    with pytest.raises(AppError) as malformed_escape:
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - non-hex X must not be accepted in an escape.
            b"utf-8''%XX", initial=True
        )
    assert malformed_escape.value == AppError(
        ExitCode.PARSE_ERROR, "malformed RFC 2231 escape"
    )

"""Adversarial source-policy authorization tests for the independent verifier."""

from __future__ import annotations

import hashlib

import pytest

from eml_attachment_remover.domain import AppError, ExitCode, Removal, RemovalReason
from eml_attachment_remover.mime_execution import Candidate, build_candidate
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_raw import RawMimeTree, RawNode, parse_raw_mime
from eml_attachment_remover.mime_validation import ContentSpec
from eml_attachment_remover.mime_verification import verify_candidate
from eml_attachment_remover.mime_verifier_policy import authorize_removals


def _mixed() -> RawMimeTree:
    return parse_raw_mime(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )


def _attachment_claim() -> Removal:
    return Removal((1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT)


def _related() -> RawMimeTree:
    return parse_raw_mime(
        b"Content-Type: multipart/related; boundary=r\r\n\r\n"
        b"--r\r\nContent-Type: text/plain\r\nContent-ID: <root@x>\r\n\r\nroot\r\n"
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <resource@x>\r\n\r\nbytes\r\n"
        b"--r--\r\n"
    )


def _related_claim() -> Removal:
    return Removal((1,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT)


@pytest.mark.parametrize(
    ("raw", "claims", "message"),
    [
        (
            b"Content-Type: application/pkcs7-signature\r\n\r\nbody",
            (),
            "protected MIME content",
        ),
        (
            b"Content-Type: text/plain\r\nContent-Disposition: form-data\r\n\r\nbody",
            (),
            "unknown MIME disposition",
        ),
        (
            b"Content-Type: text/plain; name=body.txt\r\n\r\nbody",
            (),
            "filename-only body role is ambiguous",
        ),
        (
            b"Content-Type: text/plain\r\nContent-Disposition: attachment\r\n\r\nbody",
            (),
            "attachment body role conflict",
        ),
        (
            b"Content-Type: application/json\r\n\r\n{}",
            (),
            "unsupported body MIME type application/json",
        ),
    ],
)
def test_source_oracle_rejects_every_unsafe_leaf_role(
    raw: bytes, claims: tuple[Removal, ...], message: str
) -> None:
    """Verifier authorization rejects unsafe source policy independently."""
    with pytest.raises(AppError) as rejected:
        authorize_removals(parse_raw_mime(raw), claims)
    assert rejected.value == AppError(ExitCode.VERIFICATION_ERROR, message)


def test_source_oracle_rejects_forged_and_incomplete_attachment_claims() -> None:
    """A planner cannot delete an inline correction or invent a removal reason."""
    tree = _mixed()
    claim = _attachment_claim()
    authorize_removals(tree, (claim,))
    forged = (
        (),
        (Removal((0,), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT),),
        (
            Removal(
                (1,),
                "application/octet-stream",
                RemovalReason.RELATED_NONROOT_COMPONENT,
            ),
        ),
        (claim, claim),
        (
            Removal(
                (99,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT
            ),
        ),
    )
    candidate = build_candidate(tree, (claim,))
    for claims in forged:
        with pytest.raises(AppError) as rejected:
            verify_candidate(tree, candidate, claims)
        assert rejected.value == AppError(
            ExitCode.VERIFICATION_ERROR, "removal authorization mismatch"
        )


def test_verifier_rejects_a_nonidempotent_candidate_after_authorization() -> None:
    """Output that still contains a source attachment cannot pass by plan fidelity."""
    tree = _mixed()
    raw = tree.raw
    with pytest.raises(AppError) as rejected:
        verify_candidate(
            tree,
            Candidate(raw, hashlib.sha256(raw).hexdigest(), ()),
            (_attachment_claim(),),
        )
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "candidate MIME verification failed"
    )


def test_verifier_requires_the_independent_idempotence_proof(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A source claim cannot substitute for successful output re-authorization."""
    tree = _mixed()
    candidate = build_candidate(tree, (_attachment_claim(),))
    calls = 0
    original = authorize_removals

    def authorize(source: RawMimeTree, claims: tuple[Removal, ...]) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            raise AppError(ExitCode.VERIFICATION_ERROR, "independent output policy")
        original(source, claims)

    monkeypatch.setattr(
        "eml_attachment_remover.mime_verification.authorize_removals", authorize
    )
    with pytest.raises(AppError) as rejected:
        verify_candidate(tree, candidate, (_attachment_claim(),))
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "candidate MIME verification failed"
    )


def test_source_oracle_covers_alternative_and_related_contexts() -> None:
    """Only a related nonroot may be pruned; alternatives retain every branch."""
    alternative = parse_raw_mime(
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
        b"--a\r\nContent-Type: text/plain\r\n\r\nplain\r\n"
        b"--a\r\nContent-Type: text/html\r\n\r\n<html>html</html>\r\n--a--\r\n"
    )
    authorize_removals(alternative, ())
    nested_alternative = parse_raw_mime(
        b"Content-Type: multipart/alternative; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\n\r\nplain\r\n--inner--\r\n"
        b"--outer\r\nContent-Type: text/html\r\n\r\n<html>html</html>\r\n--outer--\r\n"
    )
    authorize_removals(nested_alternative, ())
    related = _related()
    authorize_removals(related, (_related_claim(),))
    related.root.content_type.parameters[b"type"] = b"text/html"
    with pytest.raises(AppError) as mismatched_type:
        authorize_removals(related, (_related_claim(),))
    assert mismatched_type.value == AppError(
        ExitCode.VERIFICATION_ERROR, "related type does not match root"
    )


def test_source_oracle_rejects_each_related_selection_and_context_failure() -> None:
    """Related controls cannot swap roots, hide duplicate IDs, or remove the root."""
    no_children = _related()
    no_children.root.children.clear()
    with pytest.raises(AppError) as empty:
        authorize_removals(no_children, ())
    assert empty.value == AppError(
        ExitCode.VERIFICATION_ERROR, "empty multipart/related"
    )

    missing_start = _related()
    missing_start.root.content_type.parameters[b"start"] = b"<missing@x>"
    with pytest.raises(AppError) as missing:
        authorize_removals(missing_start, (_related_claim(),))
    assert missing.value == AppError(
        ExitCode.VERIFICATION_ERROR, "related start has no direct root"
    )

    literal_start_info = _related()
    literal_start_info.root.content_type.parameters[b"start-info"] = b"literal"
    authorize_removals(literal_start_info, (_related_claim(),))

    removed_start_info = _related()
    removed_start_info.root.content_type.parameters[b"start-info"] = b"<resource@x>"
    with pytest.raises(AppError) as removed:
        authorize_removals(removed_start_info, (_related_claim(),))
    assert removed.value == AppError(
        ExitCode.VERIFICATION_ERROR,
        "related start-info does not target a retained component",
    )


def test_source_oracle_rejects_empty_and_attachment_compound_roles() -> None:
    """Containers must preserve a body and cannot themselves be attachments."""
    only_attachment = parse_raw_mime(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )
    with pytest.raises(AppError) as only_attachment_error:
        authorize_removals(only_attachment, (_attachment_claim(),))
    assert only_attachment_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "no supported body remains"
    )

    alternative = parse_raw_mime(
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
        b"--a\r\nContent-Type: text/plain\r\n\r\nbody\r\n--a--\r\n"
    )
    alternative.root.disposition = ContentSpec("attachment", {})
    with pytest.raises(AppError) as alternative_attachment_error:
        authorize_removals(alternative, ())
    assert alternative_attachment_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "attachment alternative role conflict"
    )

    empty_alternative = parse_raw_mime(
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
        b"--a\r\nContent-Type: text/plain\r\n\r\nbody\r\n--a--\r\n"
    )
    empty_alternative.root.children.clear()
    with pytest.raises(AppError) as empty_alternative_error:
        authorize_removals(empty_alternative, ())
    assert empty_alternative_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "empty multipart/alternative"
    )

    unsupported_alternative = parse_raw_mime(
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
        b"--a\r\nContent-Type: application/json\r\n\r\n{}\r\n--a--\r\n"
    )
    with pytest.raises(AppError) as unsupported_alternative_error:
        authorize_removals(unsupported_alternative, ())
    assert unsupported_alternative_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "unsupported multipart/alternative branch"
    )

    mixed_attachment = _mixed()
    mixed_attachment.root.disposition = ContentSpec("attachment", {})
    with pytest.raises(AppError) as mixed_attachment_error:
        authorize_removals(mixed_attachment, ())
    assert mixed_attachment_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "attachment mixed role conflict"
    )

    related_attachment = _related()
    related_attachment.root.disposition = ContentSpec("attachment", {})
    with pytest.raises(AppError) as related_attachment_error:
        authorize_removals(related_attachment, ())
    assert related_attachment_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "attachment related role conflict"
    )


def test_source_oracle_rejects_duplicate_related_identifiers() -> None:
    """A related source has one canonical identity per component."""
    duplicate = _related()
    resource = duplicate.root.children[1]
    resource.headers = (
        Header(b"content-type", b"image/png", 0, 0),
        Header(b"content-id", b"<root@x>", 0, 0),
    )
    with pytest.raises(AppError) as rejected:
        authorize_removals(duplicate, (_related_claim(),))
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "duplicate canonical Content-ID"
    )


def test_source_oracle_keeps_filename_and_identifier_fields_distinct() -> None:
    """Name/filename and Content-ID/start labels have independent exact contracts."""
    inline_filename = parse_raw_mime(
        b"Content-Type: text/plain\r\n"
        b"Content-Disposition: inline; filename=body.txt\r\n\r\nbody"
    )
    authorize_removals(inline_filename, ())
    filename_only = parse_raw_mime(
        b"Content-Type: text/plain\r\n"
        b"Content-Disposition: attachment; filename=body.txt\r\n\r\nbody"
    )
    with pytest.raises(AppError) as filename_error:
        authorize_removals(filename_only, ())
    assert filename_error.value == AppError(
        ExitCode.VERIFICATION_ERROR, "filename-only body role is ambiguous"
    )

    malformed_start = _related()
    malformed_start.root.content_type.parameters[b"start"] = b"<broken"
    with pytest.raises(AppError) as start_error:
        authorize_removals(malformed_start, (_related_claim(),))
    assert start_error.value == AppError(
        ExitCode.PARSE_ERROR, "malformed multipart/related start"
    )

    malformed_identifier = _related()
    malformed_identifier.root.content_type.parameters[b"start"] = b"<root@x>"
    malformed_identifier.root.children[0].headers = (
        Header(b"content-id", b"<broken", 0, 0),
    )
    with pytest.raises(AppError) as identifier_error:
        authorize_removals(malformed_identifier, (_related_claim(),))
    assert identifier_error.value == AppError(
        ExitCode.PARSE_ERROR, "malformed Content-ID"
    )


def test_source_oracle_rejects_nested_related_start_and_accepts_casefolded_type() -> (
    None
):
    """Related start chooses direct children; type comparison is ASCII-folded."""
    nested_start = _related()
    nested = RawNode(
        (0, 0),
        0,
        0,
        0,
        (Header(b"content-id", b"<nested@x>", 0, 0),),
        ContentSpec("text/plain", {}),
        None,
        "7bit",
    )
    nested_start.root.children[0].children.append(nested)
    nested_start.root.content_type.parameters[b"start"] = b"<nested@x>"
    with pytest.raises(AppError) as non_direct:
        authorize_removals(nested_start, (_related_claim(),))
    assert non_direct.value == AppError(
        ExitCode.VERIFICATION_ERROR, "related start has no direct root"
    )

    folded_type = _related()
    folded_type.root.content_type.parameters[b"type"] = b"TEXT/PLAIN"
    authorize_removals(folded_type, (_related_claim(),))
    malformed_info = _related()
    malformed_info.root.content_type.parameters[b"start-info"] = b"<broken"
    with pytest.raises(AppError) as malformed:
        authorize_removals(malformed_info, (_related_claim(),))
    assert malformed.value == AppError(
        ExitCode.PARSE_ERROR, "malformed related start-info"
    )

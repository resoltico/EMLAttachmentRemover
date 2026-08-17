"""Adversarial system contracts for the text-only MIME transformation."""

from __future__ import annotations

import hashlib
import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path

import pytest

from eml_attachment_remover import process_file
from eml_attachment_remover.mime_text_execution import canonical_text_payload
from eml_attachment_remover.models import CliError, ExitCode
from tests.test_support import parse


def _plain(text: str = "PUBLIC SELECTED BODY") -> EmailMessage:
    """Return one undisposed plain-text body leaf.

    Returns:
        The synthetic plain-text body.

    """
    part = EmailMessage()
    part.set_content(text)
    return part


def _html(html: str) -> EmailMessage:
    """Return one HTML body leaf.

    Returns:
        The synthetic HTML body.

    """
    part = EmailMessage()
    part.set_content(html, subtype="html")
    return part


def _alternative(*children: EmailMessage) -> EmailMessage:
    """Return an alternative container with the supplied representations.

    Returns:
        The populated alternative container.

    """
    part = EmailMessage()
    part.make_alternative()
    for child in children:
        part.attach(child)
    return part


def _mixed(*children: EmailMessage) -> EmailMessage:
    """Return a mixed container with the supplied children.

    Returns:
        The populated mixed container.

    """
    part = EmailMessage()
    part.make_mixed()
    for child in children:
        part.attach(child)
    return part


def _related(*children: EmailMessage) -> EmailMessage:
    """Return a related aggregate with the supplied children.

    Returns:
        The populated related aggregate.

    """
    part = EmailMessage()
    part.make_related()
    for child in children:
        part.attach(child)
    return part


def _binary(
    payload: bytes = b"PUBLIC BINARY PAYLOAD",
    *,
    content_id: str | None = None,
    disposition: str | None = None,
    filename: str | None = None,
) -> EmailMessage:
    """Return one synthetic application payload with selected MIME metadata.

    Returns:
        The configured binary MIME leaf.

    """
    part = EmailMessage()
    part.set_content(
        payload,
        maintype="application",
        subtype="octet-stream",
        cte="base64",
    )
    if content_id is not None:
        part["Content-ID"] = content_id
    if disposition is not None:
        if filename is None:
            part["Content-Disposition"] = disposition
        else:
            part.add_header("Content-Disposition", disposition, filename=filename)
    elif filename is not None:
        part.set_param("name", filename, header="Content-Type")
    return part


def _assert_canonical_output(message: EmailMessage) -> EmailMessage:
    """Require exactly one unfiled, undisposed, unaddressed plain leaf.

    Returns:
        The sole verified plain-text leaf.

    """
    leaves = [part for part in message.walk() if not part.is_multipart()]
    assert len(leaves) == 1
    leaf = leaves[0]
    assert leaf.get_content_type() == "text/plain"
    assert leaf.get_filename() is None
    assert leaf.get_content_disposition() is None
    assert leaf.get("Content-ID") is None
    assert leaf.get("Content-Location") is None
    assert all(part.get_filename() is None for part in message.walk())
    assert all(part.get_content_disposition() is None for part in message.walk())
    assert leaf is message
    return leaf


def _rebasing_fixture() -> EmailMessage:
    """Build a body whose selected source path changes in the output tree.

    Returns:
        A nested message with one body resource and one ordinary attachment.

    """
    attachment = _binary(
        b"PUBLIC ORDINARY ATTACHMENT",
        disposition="attachment",
        filename="public.bin",
    )
    resource = _binary(
        b"PUBLIC RELATED RESOURCE",
        content_id="<public-resource@example.test>",
        disposition="inline",
        filename="public-resource.bin",
    )
    body = _alternative(
        _html('<p>PUBLIC HTML<img src="cid:public-resource@example.test"></p>'),
        _plain(),
    )
    body["Content-ID"] = "<public-body@example.test>"
    related = _related(resource, body)
    related.set_param("start", "<public-body@example.test>")
    related.set_param("type", "multipart/alternative")
    return _mixed(attachment, related)


def test_plan_execution_rebases_paths_and_second_pass_is_byte_exact() -> None:
    """Bind reports to source paths while verifying the rebased stored tree."""
    raw = _rebasing_fixture().as_bytes(policy=policy.SMTP)
    selected_source = b"PUBLIC SELECTED BODY\n"
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        second = base / "second.eml"
        source.write_bytes(raw)

        dry = process_file(source, output, force=False, dry_run=True)
        actual = process_file(source, output, force=False, dry_run=False)

        assert (
            actual.selected_plain_text_bodies,
            actual.discarded_body_representations,
            actual.discarded_body_resources,
            actual.removed_attachments,
            actual.warnings,
        ) == (
            dry.selected_plain_text_bodies,
            dry.discarded_body_representations,
            dry.discarded_body_resources,
            dry.removed_attachments,
            dry.warnings,
        )
        assert actual.selected_plain_text_bodies[0].path == (1, 1, 1)
        assert [item.path for item in actual.discarded_body_representations] == [
            (1, 1, 0)
        ]
        assert [item.path for item in actual.discarded_body_resources] == [(1, 0)]
        assert actual.discarded_body_resources[0].referenced_by == ((1, 1, 0),)
        assert [item.path for item in actual.removed_attachments] == [(0,)]
        assert source.read_bytes() == raw
        assert canonical_text_payload(_assert_canonical_output(parse(output))) == (
            selected_source
        )

        repeated = process_file(output, second, force=False, dry_run=False)

        assert not repeated.discarded_body_representations
        assert not repeated.discarded_body_resources
        assert not repeated.removed_attachments
        assert second.read_bytes() == output.read_bytes()


def test_discarded_attachment_may_share_the_selected_payload_digest(
    tmp_path: Path,
) -> None:
    """Verify structural removal when selected and discarded bytes are identical."""
    selected = _plain("PUBLIC IDENTICAL PAYLOAD")
    shared_payload = canonical_text_payload(selected)
    attachment = _binary(
        shared_payload,
        disposition="attachment",
        filename="duplicate.bin",
    )
    discarded_payload = attachment.get_payload(decode=True)
    assert isinstance(discarded_payload, bytes)
    shared_sha256 = hashlib.sha256(shared_payload).digest()
    assert hashlib.sha256(discarded_payload).digest() == shared_sha256
    message = _mixed(selected, attachment)
    raw = message.as_bytes(policy=policy.SMTP)
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    source.write_bytes(raw)

    result = process_file(source, output, force=False, dry_run=False)

    assert [part.filename for part in result.removed_attachments] == ["duplicate.bin"]
    retained = _assert_canonical_output(parse(output))
    assert hashlib.sha256(canonical_text_payload(retained)).digest() == shared_sha256
    assert source.read_bytes() == raw


def test_unfiled_html_mixed_sibling_is_ambiguous_and_fails_closed() -> None:
    """Never let an unclassified HTML sibling survive a successful result."""
    message = _mixed(_plain(), _html("<p>PUBLIC AMBIGUOUS HTML</p>"))
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        source.write_bytes(raw)

        with pytest.raises(CliError) as raised:
            process_file(source, output, force=False, dry_run=False)

        assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
        assert "ambiguous non-body role" in raised.value.message
        assert source.read_bytes() == raw
        assert not output.exists()


def test_empty_related_resource_is_a_real_action_and_cannot_hide_as_noop() -> None:
    """Keep action cardinality independent from descendant-leaf cardinality."""
    empty_resource = EmailMessage()
    empty_resource.make_mixed()
    message = _related(_plain(), empty_resource)
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, output, force=False, dry_run=False)

        # Python's wire parser represents an empty multipart as one empty leaf;
        # the action must still prevent the original related sibling surviving.
        assert [item.path for item in result.discarded_body_resources] == [(1, 0)]
        assert result.discarded_body_resources[0].content_type == "text/plain"
        assert output.read_bytes() != raw
        _assert_canonical_output(parse(output))


def test_related_resource_records_all_actual_body_reference_edges() -> None:
    """Represent many-to-many dependencies without inventing one owner."""
    shared = _binary(
        content_id="<shared-resource@example.test>",
        disposition="inline",
    )
    body = _alternative(
        _html('<img src="cid:shared-resource@example.test">'),
        _plain(),
        _html('<img src="cid:shared-resource@example.test" alt="PUBLIC">'),
    )
    message = _related(body, shared)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))

        result = process_file(source, None, force=False, dry_run=True)

    assert [item.path for item in result.discarded_body_representations] == [
        (0, 0),
        (0, 2),
    ]
    assert result.discarded_body_resources[0].referenced_by == ((0, 0), (0, 2))


def _message_wrapper(child: EmailMessage) -> EmailMessage:
    """Return one message/rfc822 wrapper for logical-scope adversaries.

    Returns:
        The wrapper containing one nested logical message.

    """
    wrapper = EmailMessage()
    wrapper.set_type("message/rfc822")
    wrapper.set_payload([child])
    return wrapper


def test_attached_message_references_cannot_bind_an_outer_resource() -> None:
    """Keep inner HTML identifiers inside their logical-message scope."""
    inner = _html('<img src="cid:shared@example.test">')
    body = _alternative(_plain(), _message_wrapper(inner))
    outer_resource = _binary(
        content_id="<shared@example.test>",
        disposition="inline",
    )
    message = _related(body, outer_resource)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))
        result = process_file(source, None, force=False, dry_run=True)

    assert result.discarded_body_resources[0].path == (1,)
    assert result.discarded_body_resources[0].referenced_by == ()


def test_outer_references_cannot_bind_inside_an_attached_message() -> None:
    """Audit a related message resource atomically rather than crossing scope."""
    body = _alternative(
        _plain(),
        _html('<img src="cid:shared@example.test">'),
    )
    inner = _binary(content_id="<shared@example.test>")
    message = _related(body, _message_wrapper(inner))
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))
        result = process_file(source, None, force=False, dry_run=True)

    assert [item.path for item in result.discarded_body_resources] == [(1,)]
    assert result.discarded_body_resources[0].content_type == "message/rfc822"
    assert result.discarded_body_resources[0].referenced_by == ()


def _signed_representation() -> EmailMessage:
    """Return an opaque signed representation with an inspectable descendant.

    Returns:
        A synthetic multipart/signed representation.

    """
    signed = EmailMessage()
    signed.set_type("multipart/signed")
    signed.set_param("protocol", "application/pgp-signature")
    signature = EmailMessage()
    signature.set_type("application/pgp-signature")
    signature.set_payload("PUBLIC SIGNATURE")
    signed.set_payload([_html("<p>PUBLIC SIGNED HTML</p>"), signature])
    return signed


def test_unselected_protected_representation_is_discarded_atomically() -> None:
    """Discard protected alternatives at their root without descendant records."""
    message = _alternative(_plain(), _signed_representation())
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))

        result = process_file(source, output, force=False, dry_run=False)

        assert [item.path for item in result.discarded_body_representations] == [(1,)]
        assert not result.discarded_body_resources
        _assert_canonical_output(parse(output))


def test_unmarked_protected_mixed_sibling_is_ambiguous_and_fails() -> None:
    """Fail when protected content cannot be proven to be a discardable file."""
    protected = EmailMessage()
    protected.set_type("application/pkcs7-mime")
    protected.set_payload("PUBLIC OPAQUE DATA")
    message = _mixed(_plain(), protected)
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        source.write_bytes(raw)

        with pytest.raises(CliError) as raised:
            process_file(source, output, force=False, dry_run=False)

        assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
        assert source.read_bytes() == raw
        assert not output.exists()


def test_file_like_container_cannot_be_selected_as_the_body_closure() -> None:
    """Apply non-file body requirements to retained ancestors as well as leaves."""
    message = _alternative(_plain(), _html("<p>PUBLIC HTML</p>"))
    message.add_header("Content-Disposition", "attachment", filename="body.mime")
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))

        with pytest.raises(CliError) as raised:
            process_file(source, None, force=False, dry_run=True)

    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE

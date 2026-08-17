# ruff: file-ignore[private-member-access]
"""Close defensive parse, fingerprint, and verification boundaries for v2."""

from __future__ import annotations

import hashlib
import io
import tempfile
from collections import Counter
from email import errors
from email.message import EmailMessage
from email.parser import BytesParser
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from eml_attachment_remover import mime_serialization
from eml_attachment_remover.models import CliError, ExitCode


def _nested_message(depth: int) -> EmailMessage:
    """Return one MIME tree whose sole leaf is at the requested depth.

    Returns:
        The synthetic nested message.

    """
    child = EmailMessage()
    child.set_content("PUBLIC BODY")
    for _index in range(depth):
        parent = EmailMessage()
        parent.make_mixed()
        parent.attach(child)
        child = parent
    return child


def test_depth_validation_rejects_the_first_unsupported_path() -> None:
    """Reject a tree beyond the iterative parser-safety boundary."""
    mime_serialization._validate_mime_depth(
        _nested_message(mime_serialization.MAX_MIME_DEPTH),
    )
    message = _nested_message(mime_serialization.MAX_MIME_DEPTH + 1)

    with pytest.raises(CliError) as raised:
        mime_serialization._validate_mime_depth(message)

    assert raised.value.code is ExitCode.PARSE_ERROR
    path = ".".join("1" for _index in range(101))
    assert raised.value.message == (
        "MIME tree exceeds maximum supported depth 100 at MIME path " + path
    )


def test_payload_bytes_covers_raw_bytes_and_surrogate_text_fallbacks() -> None:
    """Preserve both non-decoded payload representations deterministically."""
    raw_bytes = MagicMock()
    raw_bytes.get_payload.side_effect = [None, b"PUBLIC RAW BYTES"]
    raw_text = MagicMock()
    raw_text.get_payload.side_effect = [None, "PUBLIC\udcff"]
    unsupported = MagicMock()
    unsupported.get_payload.side_effect = [None, object()]

    assert mime_serialization._payload_bytes(raw_bytes) == b"PUBLIC RAW BYTES"
    assert mime_serialization._payload_bytes(raw_text) == b"PUBLIC\xff"
    assert mime_serialization._payload_bytes(unsupported) == b""


def test_binary_leaf_fingerprint_does_not_apply_text_newline_normalization() -> None:
    """Hash non-text bytes exactly while keeping their root path metadata."""
    message = EmailMessage()
    message.set_content(
        b"PUBLIC\r\nBINARY\rPAYLOAD",
        maintype="application",
        subtype="octet-stream",
    )
    digest = hashlib.sha256(b"PUBLIC\r\nBINARY\rPAYLOAD").hexdigest()

    assert mime_serialization._leaf_fingerprints(message) == Counter({
        ((), "application/octet-stream", None, None, None, digest): 1,
    })


def test_lesser_parser_defect_is_retained_as_an_exact_warning() -> None:
    """Keep nonstructural parser defects visible without rejecting the message."""
    message = EmailMessage()
    message.defects.append(errors.InvalidHeaderDefect("PUBLIC DEFECT"))

    assert mime_serialization._validate_defects(message) == (
        "parser reported InvalidHeaderDefect at MIME path root",
    )
    child = EmailMessage()
    child.defects.append(errors.InvalidHeaderDefect("PUBLIC CHILD DEFECT"))
    nested = EmailMessage()
    nested.make_mixed()
    nested.attach(EmailMessage())
    nested.attach(child)
    assert mime_serialization._validate_defects(nested) == (
        "parser reported InvalidHeaderDefect at MIME path 2",
    )


@pytest.mark.parametrize(
    ("failure", "diagnostic"),
    [
        (RecursionError(), "MIME nesting exceeded parser capacity"),
        (ValueError("PUBLIC PARSE FAILURE"), "PUBLIC PARSE FAILURE"),
    ],
)
def test_parse_message_wraps_each_parser_failure(
    failure: Exception,
    diagnostic: str,
) -> None:
    """Map parser capacity and value failures to stable parse errors."""
    with (
        patch.object(
            BytesParser,
            "parsebytes",
            side_effect=failure,
        ),
        pytest.raises(CliError) as raised,
    ):
        mime_serialization._parse_message(b"PUBLIC", "public.eml")

    assert raised.value.code is ExitCode.PARSE_ERROR
    assert diagnostic in raised.value.message


def test_serializer_uses_the_exact_output_policy_and_envelope_contract() -> None:
    """Bind generator construction, header folding, and Unix-from handling."""
    message = EmailMessage()
    message.set_unixfrom("From public@example.test")
    output = io.BytesIO()
    generator = MagicMock()
    with patch.object(
        mime_serialization,
        "BytesGenerator",
        return_value=generator,
    ) as generator_type:
        mime_serialization._serialize_to_stream(message, output)

    generator_type.assert_called_once_with(
        output,
        policy=mime_serialization.OUTPUT_POLICY,
        maxheaderlen=0,
    )
    generator.flatten.assert_called_once_with(message, unixfrom=True)


def test_verifier_wraps_a_generated_message_parse_failure() -> None:
    """Promote post-write parse errors to the verification error boundary."""
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "output.eml"
        temporary.write_bytes(b"PUBLIC")
        failure = CliError(ExitCode.PARSE_ERROR, "PUBLIC GENERATED PARSE FAILURE")
        with (
            patch.object(mime_serialization, "_parse_message", side_effect=failure),
            pytest.raises(CliError) as raised,
        ):
            mime_serialization._verify_serialized_message(temporary, Counter())

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "generated EML failed MIME verification: PUBLIC GENERATED PARSE FAILURE"
    )


def test_verifier_read_failure_preserves_the_operating_system_diagnostic() -> None:
    """Keep the unavailable generated-output path visible at code eight."""
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "missing-output.eml"
        with pytest.raises(CliError) as raised:
            mime_serialization._verify_serialized_message(temporary, Counter())

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message.startswith(
        "could not read generated output for verification: ",
    )
    assert "missing-output.eml" in raised.value.message


def test_verifier_rejects_a_retained_payload_fingerprint_mismatch() -> None:
    """Reject output before semantic planning when retained bytes differ."""
    message = EmailMessage()
    message.set_content("PUBLIC BODY")
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "output.eml"
        temporary.write_bytes(b"PUBLIC")
        with (
            patch.object(
                mime_serialization,
                "_parse_message",
                return_value=(message, ()),
            ),
            pytest.raises(CliError) as raised,
        ):
            mime_serialization._verify_serialized_message(temporary, Counter())

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "generated EML did not preserve every retained MIME payload"
    )


def test_verifier_rejects_a_remaining_alternative_action() -> None:
    """Require successful serialized output to be a second-pass plan no-op."""
    message = EmailMessage()
    message.set_content("PUBLIC PLAIN BODY")
    message.add_alternative("<p>PUBLIC HTML BODY</p>", subtype="html")
    raw = message.as_bytes(policy=mime_serialization.OUTPUT_POLICY)
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "output.eml"
        temporary.write_bytes(raw)
        parsed, _warnings = mime_serialization._parse_message(raw, str(temporary))
        expected = mime_serialization._leaf_fingerprints(parsed)
        structure = mime_serialization._structure_fingerprint(parsed)

        with pytest.raises(CliError) as raised:
            mime_serialization._verify_serialized_message(
                temporary,
                expected,
                structure,
            )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "generated EML is not a canonical root text/plain message"
    )


def test_verifier_rejects_a_singleton_body_wrapper_without_discards() -> None:
    """Treat nested body promotion itself as a required serialization change."""
    plain = EmailMessage()
    plain.set_content("PUBLIC PLAIN BODY")
    message = EmailMessage()
    message.make_alternative()
    message.attach(plain)
    raw = message.as_bytes(policy=mime_serialization.OUTPUT_POLICY)
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "output.eml"
        temporary.write_bytes(raw)
        parsed, _warnings = mime_serialization._parse_message(raw, str(temporary))
        expected = mime_serialization._leaf_fingerprints(parsed)
        structure = mime_serialization._structure_fingerprint(parsed)

        with pytest.raises(CliError) as raised:
            mime_serialization._verify_serialized_message(
                temporary,
                expected,
                structure,
            )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "generated EML is not a canonical root text/plain message"
    )

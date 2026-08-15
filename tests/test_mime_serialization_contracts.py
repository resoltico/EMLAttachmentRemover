# ruff: file-ignore[no-self-use, private-member-access]
"""Exact direct contracts for MIME parsing and output verification."""

from __future__ import annotations

import hashlib
import io
import tempfile
import unittest
from collections import Counter
from email import errors, policy
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from eml_attachment_remover import (
    mime_policy,
    mime_references,
    mime_serialization,
    process_file,
)
from eml_attachment_remover.models import (
    CliError,
    ExitCode,
    PartContext,
    ReferenceIndex,
    RemovedPart,
)
from tests.test_internal_support import attachment


def _mixed(*children: EmailMessage) -> EmailMessage:
    """Return a multipart/mixed message containing the supplied children.

    Returns:
        A new multipart message with the children in their supplied order.

    """
    message = EmailMessage()
    message.make_mixed()
    for child in children:
        message.attach(child)
    return message


def _nested_multipart(depth: int) -> EmailMessage:
    """Return a synthetic MIME tree with one child at each requested level.

    Returns:
        A root whose leaf path length equals ``depth``.

    """
    child = EmailMessage()
    child.set_content("public leaf")
    for _index in range(depth):
        parent = EmailMessage()
        parent.make_mixed()
        parent.attach(child)
        child = parent
    return child


class TestSerializationContracts(unittest.TestCase):
    """Specify transfer decoding, defects, generator options, and verification."""

    def test_payload_bytes_explicitly_requests_transfer_decoding(self) -> None:
        part = MagicMock()
        part.get_payload.return_value = b"decoded"

        assert mime_serialization._payload_bytes(part) == b"decoded"
        part.get_payload.assert_called_once_with(decode=True)

    def test_payload_bytes_preserves_surrogateescaped_text(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, "public\udcff"]

        assert mime_serialization._payload_bytes(part) == b"public\xff"

    def test_leaf_fingerprints_count_duplicates_and_normalize_text_newlines(
        self,
    ) -> None:
        first = EmailMessage()
        first.set_content("line one\nline two")
        second = EmailMessage()
        second.set_content("line one\nline two")
        expected_digest = hashlib.sha256(b"line one\nline two\n").hexdigest()

        assert mime_serialization._leaf_fingerprints(
            _mixed(first, second),
        ) == Counter({
            ((0,), "text/plain", None, None, None, expected_digest): 1,
            ((1,), "text/plain", None, None, None, expected_digest): 1,
        })

    def test_nested_defects_report_exact_zero_based_tree_paths(self) -> None:
        child = EmailMessage()
        child.defects.append(errors.InvalidHeaderDefect("child"))
        message = _mixed(EmailMessage(), child)
        root_defect = errors.InvalidHeaderDefect("root")
        message.defects.append(root_defect)

        found = mime_serialization._iter_defects(message)

        assert found == [((), root_defect), ((1,), child.defects[0])]
        assert mime_serialization._validate_defects(message) == (
            "parser reported InvalidHeaderDefect at MIME path root",
            "parser reported InvalidHeaderDefect at MIME path 2",
        )

    def test_unsafe_defect_reports_exact_type_and_path(self) -> None:
        child = EmailMessage()
        child.defects.append(errors.NoBoundaryInMultipartDefect("broken"))
        message = _mixed(child)

        with pytest.raises(CliError) as raised:
            mime_serialization._validate_defects(message)

        assert raised.value.code is ExitCode.PARSE_ERROR
        assert raised.value.message == (
            "unsafe MIME structure or transfer encoding: "
            "NoBoundaryInMultipartDefect at MIME path 1"
        )

    def test_parse_failure_names_the_source_and_original_error(self) -> None:
        with patch(
            "eml_attachment_remover.mime_serialization.BytesParser.parsebytes",
            side_effect=ValueError("public parse failure"),
        ):
            with pytest.raises(CliError) as raised:
                mime_serialization._parse_message(b"public", "public.eml")

        assert raised.value.code is ExitCode.PARSE_ERROR
        assert (
            raised.value.message == "could not parse public.eml: public parse failure"
        )

    def test_mime_depth_accepts_limit_and_rejects_first_excess_level(self) -> None:
        mime_serialization._validate_mime_depth(
            _nested_multipart(mime_serialization.MAX_MIME_DEPTH),
        )
        with pytest.raises(CliError) as raised:
            mime_serialization._validate_mime_depth(
                _nested_multipart(mime_serialization.MAX_MIME_DEPTH + 1),
            )

        path = ".".join("1" for _index in range(101))
        assert raised.value.code is ExitCode.PARSE_ERROR
        assert raised.value.message == (
            "MIME tree exceeds maximum supported depth 100 at MIME path " + path
        )

    def test_all_recursive_output_operations_accept_the_depth_limit(self) -> None:
        message = _nested_multipart(mime_serialization.MAX_MIME_DEPTH)
        output = io.BytesIO()

        references = mime_references._collect_references(message)
        state = mime_policy._remove_attachments(message, references)
        fingerprints = mime_serialization._leaf_fingerprints(message)
        structure = mime_serialization._structure_fingerprint(message)
        mime_serialization._serialize_to_stream(message, output)

        assert not state.removed
        assert len(fingerprints) == 1
        assert len(structure) == mime_serialization.MAX_MIME_DEPTH + 1
        assert output.getvalue()

    def test_process_file_accepts_limit_and_rejects_excess_before_output(self) -> None:
        accepted = _nested_multipart(mime_serialization.MAX_MIME_DEPTH)
        accepted.attach(attachment("public.pdf"))
        rejected = _nested_multipart(mime_serialization.MAX_MIME_DEPTH + 1)
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            accepted_source = base / "accepted.eml"
            accepted_output = base / "accepted-output.eml"
            rejected_source = base / "rejected.eml"
            rejected_output = base / "rejected-output.eml"
            accepted_source.write_bytes(accepted.as_bytes(policy=policy.SMTP))
            rejected_source.write_bytes(rejected.as_bytes(policy=policy.SMTP))

            result = process_file(
                accepted_source,
                accepted_output,
                force=False,
                dry_run=False,
            )
            with pytest.raises(CliError) as raised:
                process_file(
                    rejected_source,
                    rejected_output,
                    force=False,
                    dry_run=False,
                )

        assert [part.filename for part in result.removed] == ["public.pdf"]
        assert raised.value.code is ExitCode.PARSE_ERROR
        assert "maximum supported depth 100" in raised.value.message
        assert not rejected_output.exists()

    def test_parser_recursion_exhaustion_is_a_parse_error(self) -> None:
        with patch(
            "eml_attachment_remover.mime_serialization.BytesParser.parsebytes",
            side_effect=RecursionError,
        ):
            with pytest.raises(CliError) as raised:
                mime_serialization._parse_message(b"public", "deep.eml")

        assert raised.value.code is ExitCode.PARSE_ERROR
        assert raised.value.message == (
            "could not parse deep.eml: MIME nesting exceeded parser capacity"
        )

    def test_serializer_uses_the_exact_wire_contract(self) -> None:
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

    def test_verification_root_context_has_exact_root_semantics(self) -> None:
        references = ReferenceIndex(frozenset(), frozenset({"public.png"}))

        assert mime_serialization._root_context(references) == PartContext(
            path=(),
            parent_type=None,
            under_related=False,
            references=references,
            is_root=True,
        )

    def test_remaining_root_attachment_reports_all_metadata(self) -> None:
        message = attachment("root.bin")
        message["Content-Type"] = "application/octet-stream"

        assert mime_serialization._remaining_removable_part(message) == RemovedPart(
            (),
            "application/octet-stream",
            "root.bin",
            "attachment",
        )

    def test_verification_read_failure_has_exact_diagnostic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "missing.eml"
            with pytest.raises(CliError) as raised:
                mime_serialization._verify_serialized_message(temporary, Counter())

        assert raised.value.code is ExitCode.VERIFICATION_ERROR
        assert raised.value.message.startswith(
            "could not read generated output for verification:",
        )
        assert "missing.eml" in raised.value.message

    def test_verification_wraps_parse_diagnostic_exactly(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            parse_error = CliError(ExitCode.PARSE_ERROR, "public parse failure")
            with patch.object(
                mime_serialization,
                "_parse_message",
                side_effect=parse_error,
            ):
                with pytest.raises(CliError) as raised:
                    mime_serialization._verify_serialized_message(temporary, Counter())

        assert raised.value.message == (
            "generated EML failed MIME verification: public parse failure"
        )

    def test_verification_reports_exact_payload_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            with (
                patch.object(
                    mime_serialization,
                    "_parse_message",
                    return_value=(EmailMessage(), ()),
                ),
                patch.object(
                    mime_serialization,
                    "_leaf_fingerprints",
                    return_value=Counter({("changed",): 1}),
                ),
            ):
                with pytest.raises(CliError) as raised:
                    mime_serialization._verify_serialized_message(temporary, Counter())

        assert raised.value.message == (
            "generated EML did not preserve every retained MIME payload"
        )

    def test_verification_names_an_unnamed_remaining_entity(self) -> None:
        unnamed = RemovedPart((), "application/octet-stream", None, "attachment")
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            with (
                patch.object(
                    mime_serialization,
                    "_parse_message",
                    return_value=(EmailMessage(), ()),
                ),
                patch.object(
                    mime_serialization,
                    "_leaf_fingerprints",
                    return_value=Counter(),
                ),
                patch.object(
                    mime_serialization,
                    "_remaining_removable_part",
                    return_value=unnamed,
                ),
            ):
                with pytest.raises(CliError) as raised:
                    mime_serialization._verify_serialized_message(temporary, Counter())

        assert raised.value.message == (
            "generated EML still contains removable attachment 'unnamed MIME entity'"
        )

# ruff: file-ignore[private-member-access]
"""Regression tests for output verification and atomic publication."""

from __future__ import annotations

import os
import tempfile
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from eml_attachment_remover import (
    atomic_publish,
    mime_serialization,
    models,
    output_commit,
    storage,
)


@pytest.mark.parametrize(
    ("header", "changed_value"),
    [
        ("Content-Type", "multipart/alternative"),
        ("Subject", "PUBLIC CHANGED"),
    ],
)
def test_structure_verification_rejects_parent_type_and_header_changes(
    header: str,
    changed_value: str,
) -> None:
    body = EmailMessage()
    body.set_content("PUBLIC BODY")
    original = EmailMessage()
    original["Subject"] = "PUBLIC ORIGINAL"
    original.make_mixed()
    original.attach(body)
    expected_leaves = mime_serialization._leaf_fingerprints(original)
    expected_structure = mime_serialization._structure_fingerprint(original)
    original.replace_header(header, changed_value)

    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / "changed.eml"
        output.write_bytes(original.as_bytes())
        with pytest.raises(models.CliError) as raised:
            mime_serialization._verify_serialized_message(
                output,
                expected_leaves,
                expected_structure,
            )

    assert raised.value.code is models.ExitCode.VERIFICATION_ERROR
    assert "structure and headers" in raised.value.message


def test_structure_verification_rejects_preamble_or_epilogue_changes() -> None:
    body = EmailMessage()
    body.set_content("PUBLIC BODY")
    original = EmailMessage()
    original.make_mixed()
    original.attach(body)
    original.preamble = "PUBLIC PREAMBLE\r\n"
    original.epilogue = "PUBLIC EPILOGUE\r\n"
    expected_leaves = mime_serialization._leaf_fingerprints(original)
    expected_structure = mime_serialization._structure_fingerprint(original)
    original.preamble = "PUBLIC CHANGED PREAMBLE\n"

    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / "changed.eml"
        output.write_bytes(original.as_bytes())
        with pytest.raises(models.CliError) as raised:
            mime_serialization._verify_serialized_message(
                output,
                expected_leaves,
                expected_structure,
            )

    assert raised.value.code is models.ExitCode.VERIFICATION_ERROR


def test_structure_fingerprint_normalizes_transport_newlines() -> None:
    body = EmailMessage()
    body.set_content("PUBLIC BODY")
    message = EmailMessage()
    message.make_mixed()
    message.attach(body)
    message.preamble = "PUBLIC PREAMBLE\n"
    message.epilogue = "PUBLIC EPILOGUE\n"
    first = mime_serialization._structure_fingerprint(message)
    message.preamble = "PUBLIC PREAMBLE\r\n"
    message.epilogue = "PUBLIC EPILOGUE\r\n"

    assert mime_serialization._structure_fingerprint(message) == first


def test_unchanged_verifier_rejects_injected_corruption() -> None:
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "temporary.eml"
        temporary.write_bytes(b"PUBLIC CHANGED")

        with pytest.raises(models.CliError) as raised:
            storage._verify_unchanged_output(temporary, b"PUBLIC ORIGINAL")

    assert raised.value.code is models.ExitCode.VERIFICATION_ERROR


def test_unchanged_verifier_maps_read_failure_to_verification_error() -> None:
    with (
        patch.object(Path, "read_bytes", side_effect=OSError("PUBLIC READ BLOCKED")),
        pytest.raises(models.CliError) as raised,
    ):
        storage._verify_unchanged_output(Path("public.eml"), b"PUBLIC")

    assert raised.value.code is models.ExitCode.VERIFICATION_ERROR
    assert "PUBLIC READ BLOCKED" in raised.value.message


def test_fingerprint_tolerates_defensive_multipart_without_list_payload() -> None:
    malformed = MagicMock(spec=EmailMessage)
    malformed.is_multipart.return_value = True
    malformed.get_payload.return_value = "PUBLIC MALFORMED PAYLOAD"

    assert not mime_serialization._leaf_fingerprints(malformed)


def test_posix_publication_removes_completed_temporary() -> None:
    if os.name == "nt":
        return
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        temporary = base / "temporary.eml"
        destination = base / "destination.eml"
        temporary.write_bytes(b"PUBLIC VERIFIED")

        output_commit._publish_without_clobber(temporary, destination)

        assert not temporary.exists()
        assert destination.read_bytes() == b"PUBLIC VERIFIED"


def test_post_publication_cleanup_failure_reports_private_temp_path() -> None:
    temporary = Path("public-temporary.eml")
    destination = Path("public-output.eml")
    with (
        patch.object(atomic_publish, "_native_no_replace", return_value=False),
        patch.object(Path, "hardlink_to"),
        patch.object(Path, "unlink", side_effect=OSError("PUBLIC CLEANUP BLOCKED")),
        pytest.raises(models.CliError) as raised,
    ):
        output_commit._publish_without_clobber(temporary, destination)

    assert raised.value.code is models.ExitCode.WRITE_ERROR
    assert str(temporary) in raised.value.message
    assert "published verified output" in raised.value.message


def test_windows_no_clobber_mode_uses_nonreplacing_rename() -> None:
    temporary = Path("public-temporary.eml")
    destination = Path("public-output.eml")
    with (
        patch.object(atomic_publish.os, "name", new="nt"),  # type: ignore[attr-defined]
        patch.object(Path, "rename") as rename,
        patch.object(Path, "hardlink_to") as hardlink,
    ):
        output_commit._publish_without_clobber(temporary, destination)

    rename.assert_called_once_with(destination)
    hardlink.assert_not_called()


def test_windows_existing_target_is_reported_as_a_race_conflict() -> None:
    temporary = Path("public-temporary.eml")
    destination = Path("public-output.eml")
    with (
        patch.object(atomic_publish.os, "name", new="nt"),  # type: ignore[attr-defined]
        patch.object(Path, "rename", side_effect=FileExistsError("PUBLIC OCCUPIED")),
        pytest.raises(models.CliError) as raised,
    ):
        output_commit._publish_without_clobber(temporary, destination)

    assert raised.value.code is models.ExitCode.OUTPUT_CONFLICT

# ruff: file-ignore[private-member-access]
"""Exact mutation-sensitive contracts for zipapp and smoke tooling."""

from __future__ import annotations

import os
import stat
import tempfile
from email import policy
from email.parser import BytesParser
from pathlib import Path
from unittest.mock import call, patch

import pytest
from tools import build_zipapp, smoke_distribution


def test_zipapp_parser_preserves_every_public_option_contract() -> None:
    """Keep exact defaults, conversions, actions, and help strings."""
    parser = build_zipapp._build_parser()
    target = parser._option_string_actions["--target"]
    checksum = parser._option_string_actions["--checksum-file"]
    no_verify = parser._option_string_actions["--no-verify"]

    assert parser.description == build_zipapp.__doc__
    assert (target.type, target.default, target.help) == (
        Path,
        build_zipapp.DEFAULT_TARGET,
        f"archive path (default: {build_zipapp.DEFAULT_TARGET})",
    )
    assert (checksum.type, checksum.default, checksum.help) == (
        Path,
        None,
        "optional SHA-256 manifest to write for this archive",
    )
    assert (no_verify.const, no_verify.default, no_verify.help) == (
        True,
        False,
        "skip executing the completed archive with --version",
    )


def test_checksum_and_build_create_missing_parent_trees() -> None:
    """Support documented nested output paths for both generated files."""
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        archive = root / "public.pyz"
        archive.write_bytes(b"PUBLIC ARCHIVE")
        checksum = root / "missing" / "deep" / "SHA256SUMS"
        build_zipapp._write_checksum(checksum, archive)
        assert checksum.is_file()

        target = root / "other-missing" / "deep" / "public.pyz"
        with (
            patch.object(build_zipapp, "_write_archive"),
            patch.object(build_zipapp, "_verify_archive"),
            patch.object(Path, "replace"),
        ):
            build_zipapp.build_zipapp(target, verify=True)
        assert target.parent.is_dir()


def test_zipapp_lexical_normalization_allows_a_missing_parent() -> None:
    """Normalize a future nested output without requiring its parent to exist."""
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        with patch.object(Path, "cwd", return_value=root):
            actual = build_zipapp._lexical_absolute(
                Path("missing-parent") / "public.pyz"
            )

    assert actual == root / "missing-parent" / "public.pyz"


def test_checksum_rejects_symbolic_output_with_exact_context() -> None:
    """Preserve the direct checksum writer's final-component safety check."""
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        archive = base / "public.pyz"
        archive.write_bytes(b"PUBLIC")
        referent = base / "referent"
        referent.write_bytes(b"MUST REMAIN")
        checksum = base / "SHA256SUMS"
        checksum.symlink_to(referent)

        with pytest.raises(ValueError, match="symbolic link") as raised:
            build_zipapp._write_checksum(checksum, archive)

        normalized_checksum = checksum.parent.resolve() / checksum.name
        assert str(raised.value) == (
            f"checksum output must not be a symbolic link: {normalized_checksum}"
        )
        assert referent.read_bytes() == b"MUST REMAIN"


def test_checksum_cleanup_does_not_mask_the_original_failure() -> None:
    """Tolerate a concurrently removed temporary checksum after write failure."""
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        archive = base / "public.pyz"
        archive.write_bytes(b"PUBLIC")
        checksum = base / "SHA256SUMS"
        failure = RuntimeError("public checksum failure")

        def remove_then_fail(path: Path, _content: bytes) -> int:
            path.unlink()
            raise failure

        with (
            patch.object(Path, "write_bytes", remove_then_fail),
            pytest.raises(RuntimeError, match="public checksum failure"),
        ):
            build_zipapp._write_checksum(checksum, archive)


def test_build_cleanup_does_not_mask_the_original_failure() -> None:
    """Tolerate a concurrently removed temporary archive after build failure."""
    with tempfile.TemporaryDirectory() as directory:
        target = Path(directory) / "public.pyz"
        temporary = Path(directory) / ".public.pyz.public.tmp"
        temporary.write_bytes(b"PUBLIC")
        failure = RuntimeError("public archive failure")

        def remove_then_fail(
            path: Path,
            _metadata: build_zipapp.ProjectMetadata,
        ) -> None:
            path.unlink()
            raise failure

        with (
            patch.object(build_zipapp, "_temporary_path", return_value=temporary),
            patch.object(build_zipapp, "_write_archive", side_effect=remove_then_fail),
            pytest.raises(RuntimeError, match="public archive failure"),
        ):
            build_zipapp.build_zipapp(target, verify=False)


def test_build_forwards_verification_and_applies_all_executable_bits() -> None:
    """Require requested execution verification and a portable executable mode."""
    with tempfile.TemporaryDirectory() as directory:
        target = Path(directory) / "public.pyz"
        temporary = Path(directory) / ".public.pyz.public.tmp"
        temporary.write_bytes(b"PUBLIC")
        metadata = build_zipapp.ProjectMetadata(
            "public-project",
            "9.8.7",
            "Public summary",
            ">=3.14,<3.15",
            "MIT",
            "CPython",
        )
        original_mode = temporary.stat().st_mode
        with (
            patch.object(build_zipapp, "_temporary_path", return_value=temporary),
            patch.object(build_zipapp, "_project_metadata", return_value=metadata),
            patch.object(build_zipapp, "_write_archive"),
            patch.object(build_zipapp, "_verify_archive") as verify,
            patch.object(Path, "chmod") as chmod,
        ):
            built = build_zipapp.build_zipapp(target, verify=True)

        verify.assert_called_once_with(temporary, metadata, execute=True)
        chmod.assert_called_once_with(
            original_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        )
        assert built == target.parent.resolve() / target.name


def test_smoke_fixture_has_exact_text_only_transformation_contract() -> None:
    """Generate SMTP wire bytes with an alternative resource and attachment."""
    wire = smoke_distribution._fixture()
    parsed = BytesParser(policy=policy.default).parsebytes(wire)
    attachment = next(parsed.iter_attachments())
    resource = next(
        part for part in parsed.walk() if part.get_content_disposition() == "inline"
    )

    assert b"\r\n" in wire
    assert b"\n\n" not in wire
    assert parsed["Subject"] == smoke_distribution.EXPECTED_SUBJECT
    assert attachment.get_content_type() == "application/octet-stream"
    assert attachment.get_filename() == "public.bin"
    assert attachment.get_payload(decode=True) == smoke_distribution.ATTACHMENT_PAYLOAD
    assert resource.get_content_type() == "image/jpeg"
    assert resource.get_filename() == "public-image.jpg"
    assert resource.get_payload(decode=True) == smoke_distribution.BODY_RESOURCE


def test_smoke_environment_is_exact_with_and_without_pythonpath() -> None:
    """Remove optional import overrides and set only strict Python controls."""
    expected = {
        "PUBLIC_SETTING": "kept",
        "PYTHONDEVMODE": "1",
        "PYTHONNOUSERSITE": "1",
        "PYTHONWARNINGS": "error",
    }
    for source in (
        {"PUBLIC_SETTING": "kept"},
        {"PUBLIC_SETTING": "kept", "PYTHONPATH": "untrusted"},
    ):
        with patch.object(os.environ, "items", return_value=source.items()):
            assert smoke_distribution._environment() == expected


def test_smoke_main_forwards_exact_identity_and_portable_file_names() -> None:
    """Lock distribution lookup and the complete installed command invocation."""
    version_result = type(
        "PublicResult",
        (),
        {"stdout": f"{smoke_distribution.COMMAND_NAME} 9.8.7\n"},
    )()
    with (
        patch.object(smoke_distribution, "_installed_command", return_value="public"),
        patch.object(
            smoke_distribution,
            "distribution_version",
            return_value="9.8.7",
        ) as version,
        patch.object(
            smoke_distribution,
            "_run_command",
            side_effect=[version_result, version_result],
        ) as run,
        patch.object(smoke_distribution, "_verify_output") as verify,
    ):
        assert smoke_distribution.main() == 0

    version.assert_called_once_with(smoke_distribution.DISTRIBUTION_NAME)
    assert run.call_args_list[0] == call("public", "--version")
    processing_arguments = run.call_args_list[1].args
    assert processing_arguments[0:2] == ("public", "--output")
    assert Path(processing_arguments[2]).name == "output.eml"
    assert processing_arguments[3] == "--"
    assert Path(processing_arguments[4]).name == "source.eml"
    verified_source, verified_destination, original = verify.call_args.args
    assert verified_source.name == "source.eml"
    assert verified_destination.name == "output.eml"
    assert isinstance(original, bytes)


def test_smoke_main_reports_exact_version_identity_failure() -> None:
    """Keep a stable diagnostic when console and installed metadata disagree."""
    version_result = type(
        "PublicResult",
        (),
        {"stdout": f"{smoke_distribution.COMMAND_NAME} 9.8.6\n"},
    )()
    with (
        patch.object(smoke_distribution, "_installed_command", return_value="public"),
        patch.object(smoke_distribution, "distribution_version", return_value="9.8.7"),
        patch.object(smoke_distribution, "_run_command", return_value=version_result),
        pytest.raises(RuntimeError) as raised,
    ):
        smoke_distribution.main()

    assert str(raised.value) == (
        "installed console-script version does not match package metadata"
    )

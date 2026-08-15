"""Verify the public repository's filename and content policy contracts."""

from __future__ import annotations

import socket
from pathlib import Path
from unittest.mock import patch

import pytest
from tools import repository_hygiene_policy as policy


@pytest.mark.parametrize(
    "name",
    ["__pycache__", "package.egg-info"],
)
def test_generated_directory_recognizes_exact_generated_names(name: str) -> None:
    """Recognize each documented generated-directory category."""
    assert policy.generated_directory(name)


@pytest.mark.parametrize("name", ["package.EGG-INFO", "public"])
def test_generated_directory_matching_is_precise_and_case_sensitive(
    name: str,
) -> None:
    """Keep lookalike public directories in the public traversal."""
    assert not policy.generated_directory(name)


def test_generated_file_rules_distinguish_nested_and_root_output() -> None:
    """Apply root-only output rules without a falsy nested-mode argument."""
    assert not policy.generated_file("public.txt")
    assert not policy.generated_file(".coverage")
    assert policy.generated_file("public.pyc")
    assert policy.generated_root_file(".coverage", mutmut_statistic=False)
    assert policy.generated_root_file(
        "mutmut-stats.json",
        mutmut_statistic=True,
    )


@pytest.mark.parametrize(
    ("name", "kind", "diagnostic"),
    [
        (
            "__pycache__",
            "directory",
            (
                "Python bytecode cache directory is prohibited because bytecode can "
                "embed machine-specific paths; run Python with bytecode disabled and "
                "remove this path"
            ),
        ),
        (
            ".pytest_cache",
            "directory",
            (
                "repository-local tool cache directory is prohibited because cache "
                "data is not public evidence; disable caching or use ephemeral "
                "storage and remove this path"
            ),
        ),
        (
            "public.pyc",
            "file",
            (
                "Python bytecode cache is prohibited because it can embed "
                "machine-specific paths; run Python with bytecode disabled and "
                "remove this path"
            ),
        ),
        (
            ".coverage.worker",
            "file",
            (
                "repository-local coverage data is prohibited because it can embed "
                "machine-specific paths; use ephemeral coverage storage and remove "
                "this path"
            ),
        ),
    ],
)
def test_cache_reasons_are_exact_and_machine_path_aware(
    name: str,
    kind: str,
    diagnostic: str,
) -> None:
    """Explain how to prevent every recognized privacy-bearing cache."""
    actual = (
        policy.cache_directory_reason(name)
        if kind == "directory"
        else policy.cache_file_reason(name)
    )
    assert actual == diagnostic


def test_cache_reason_matching_does_not_reject_public_lookalikes() -> None:
    """Keep cache policy exact instead of using broad substring matching."""
    assert policy.cache_directory_reason("pytest-cache-guide") is None
    assert policy.cache_file_reason("coverage.xml") is None


@pytest.mark.parametrize(
    ("name", "expected"),
    [
        ("settings.env.backup", "environment or credential filename is prohibited"),
        ("_netrc_backup", "environment or credential filename is prohibited"),
        (".netrc-backup", "environment or credential filename is prohibited"),
        ("credentials-backup", "credential-bearing filename is prohibited"),
        ("credentials_backup", "credential-bearing filename is prohibited"),
        ("id_rsa-old", "private-key filename is prohibited"),
        ("id_rsa.pub", None),
    ],
)
def test_private_filename_reasons_cover_each_separator_and_public_keys(
    name: str,
    expected: str | None,
) -> None:
    """Preserve exact sensitive-name decisions and actionable diagnostics."""
    assert policy.private_file_reason(name) == expected


def test_public_content_diagnostics_are_exact_and_line_specific() -> None:
    """Report each private identity and concrete home path on its source line."""
    mac_home = "/" + "Users" + "/private/work"
    linux_home = "/" + "home" + "/private/work"
    windows_home = "C:\\" + "Users" + "\\private\\work"
    private_identity = "private@" + "example.com"
    text = (
        "public@example.test\n"
        f"{private_identity}\n"
        f"{mac_home}\n"
        f"{linux_home}\n"
        f"{windows_home}\n"
    )

    assert policy.public_content_messages(text) == (
        (
            "non-reserved email identity on line 2; replace it with an example.test "
            "identity"
        ),
        "user-home path on line 3; replace it with a portable placeholder",
        "user-home path on line 4; replace it with a portable placeholder",
        "user-home path on line 5; replace it with a portable placeholder",
    )


def test_public_content_rejects_only_the_complete_local_hostname() -> None:
    """Reject the runtime hostname without matching larger public tokens."""
    hostname = "local-machine.invalid"
    text = f"{hostname}\nprefix{hostname}suffix\nMACHINE_HOST\n"
    with patch.object(socket, "gethostname", return_value=hostname):
        assert policy.public_content_messages(text) == (
            "local hostname on line 1; replace it with MACHINE_HOST",
        )


@pytest.mark.parametrize("hostname", ["", "localhost", "LOCALHOST.LOCALDOMAIN"])
def test_generic_hostnames_do_not_create_private_identity_rules(hostname: str) -> None:
    """Permit empty and conventional non-identifying local hostnames."""
    with patch.object(socket, "gethostname", return_value=hostname):
        assert policy.public_content_messages(f"{hostname}\n") == ()


def test_generated_content_is_utf8_or_explicitly_validated_and_reports_errors(
    tmp_path: Path,
) -> None:
    """Fail closed for opaque data except an explicitly validated artifact."""
    binary = tmp_path / "binary.dat"
    binary.write_bytes(b"\xff\xfe")
    assert policy.generated_content_messages(binary) == (
        (
            "opaque generated file cannot be audited for private machine data; "
            "remove it or publish it only as an explicitly validated build/release "
            "artifact"
        ),
    )
    assert policy.generated_content_messages(binary, allow_opaque=True) == ()

    private = tmp_path / "private.log"
    private.write_bytes(b"private@" + b"example.com\n")
    assert policy.generated_content_messages(private) == (
        (
            "non-reserved email identity on line 1; replace it with an example.test "
            "identity"
        ),
    )

    with patch.object(Path, "read_bytes", side_effect=OSError("denied")):
        assert policy.generated_content_messages(private) == (
            "cannot read generated file: denied",
        )

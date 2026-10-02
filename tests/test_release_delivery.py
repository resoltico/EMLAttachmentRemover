"""Complete release delivery, atomic failure, and host-adaptation contracts."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest
from tools import macos_archive, qualify_release, release_files
from tools import release_delivery as delivery
from tools.release_files import ReleaseQualificationError


def _portable(directory: Path) -> tuple[Path, ...]:
    directory.mkdir()
    for name in (
        "remove-eml-attachments.pyz",
        "public.whl",
        "public.tar.gz",
        "SHA256SUMS",
    ):
        (directory / name).write_bytes(b"public")
    return tuple(directory.iterdir())


def _native(directory: Path) -> Path:
    directory.mkdir()
    path = directory / "native.zip"
    path.write_bytes(b"native")
    return path


@pytest.mark.parametrize("host", ["linux", "win32"])
def test_complete_release_requires_macos(host: str) -> None:
    with (
        patch.object(sys, "platform", host),
        pytest.raises(
            ReleaseQualificationError,
            match=(
                r"^Complete native release production/verification requires macOS; "
                r"use qualify_release\.py for portable-only artifacts$"
            ),
        ),
    ):
        delivery.verify(Path("unused"))


def test_verifier_requires_native_asset_and_checks_its_processor(
    tmp_path: Path,
) -> None:
    paths = (tmp_path / "SHA256SUMS",)
    with (
        patch.object(sys, "platform", "darwin"),
        patch.object(
            qualify_release, "verify_release_directory", return_value=paths
        ) as portable,
        patch.object(macos_archive, "verify") as native,
    ):
        assert delivery.verify(tmp_path) == paths
    portable.assert_called_once_with(
        tmp_path,
        additional_artifacts=(
            "eml_attachment_remover-4.0.0-macos-arm64.zip",
            "eml_attachment_remover-4.0.0-macos-x86_64.zip",
        ),
    )
    assert native.call_count == 2
    assert [call.args for call in native.call_args_list] == [
        (
            tmp_path / f"eml_attachment_remover-4.0.0-macos-{cpu}.zip",
            tmp_path / "remove-eml-attachments.pyz",
            "4.0.0",
            cpu,
        )
        for cpu in delivery.ARCHITECTURES
    ]


def test_native_build_is_fresh_and_records_no_customer_configuration(
    tmp_path: Path,
) -> None:
    directory = tmp_path / "native"
    with (
        patch.object(subprocess, "run") as run,
        patch.object(macos_archive, "package") as package,
    ):
        assert (
            delivery._native(directory, "arm64", tmp_path / "icon")
            == directory / "native.zip"
        )
    assert run.call_args.args[0] == [
        "/bin/sh",
        str(delivery.ROOT / "integrations/macos-ui/build.sh"),
        str(directory / macos_archive.APP),
        "arm64",
    ]
    assert run.call_args.kwargs["timeout"] == 600
    assert run.call_args.kwargs["check"] is True
    assert run.call_args.kwargs["env"] == {
        **os.environ,
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_ICON_RESOURCES": str(tmp_path / "icon"),
    }
    package.assert_called_once_with(
        directory / macos_archive.APP, directory / "native.zip"
    )


@pytest.mark.parametrize("failure", ["none", "mismatch", "verify"])
def test_delivery_publishes_one_complete_set_or_preserves_empty_destination(
    tmp_path: Path, failure: str
) -> None:
    output = tmp_path / "release"
    output.mkdir()

    def native(directory: Path, architecture: str, icon_resources: Path) -> Path:
        assert architecture in delivery.ARCHITECTURES
        assert icon_resources.name == "icon-resources"
        result = _native(directory)
        if failure == "mismatch" and directory.name == "arm64-b":
            result.write_bytes(b"different")
        return result

    def verify(directory: Path) -> tuple[Path, ...]:
        if failure == "verify":
            message = "Rejected candidate"
            raise ReleaseQualificationError(message)
        return tuple(sorted(directory.iterdir()))

    with (
        patch.object(sys, "platform", "darwin"),
        patch.object(qualify_release, "qualify_release", side_effect=_portable),
        patch.object(delivery, "_native", side_effect=native),
        patch.object(subprocess, "run") as compile_icon,
        patch.object(delivery, "verify", side_effect=verify),
    ):
        if failure == "none":
            paths = delivery.build(output)
            assert len(paths) == 6
            assert {path.name for path in paths} == {
                "public.whl",
                "public.tar.gz",
                "remove-eml-attachments.pyz",
                "SHA256SUMS",
                "eml_attachment_remover-4.0.0-macos-arm64.zip",
                "eml_attachment_remover-4.0.0-macos-x86_64.zip",
            }
            assert all(path.is_file() for path in paths)
            assert "macos-arm64.zip" in (output / "SHA256SUMS").read_text()
            assert "macos-x86_64.zip" in (output / "SHA256SUMS").read_text()
        else:
            message = (
                "Native archives differ between independent builds"
                if failure == "mismatch"
                else "Rejected candidate"
            )
            with pytest.raises(ReleaseQualificationError, match=r"^" + message + "$"):
                delivery.build(output)
            assert list(output.iterdir()) == []
            assert list(tmp_path.glob(".release.*")) == []
    compile_icon.assert_called_once()


def test_staged_copy_is_verified(tmp_path: Path) -> None:
    source = tmp_path / "source"
    source.write_bytes(b"public")
    staging = tmp_path / "staging"
    staging.mkdir()
    with (
        patch.object(release_files, "sha256", side_effect=["one", "two"]),
        pytest.raises(
            ReleaseQualificationError, match=r"^Staged delivery copy changed$"
        ),
    ):
        delivery._copy_verified((source,), staging)


@pytest.mark.parametrize(
    ("mode", "portable"),
    [
        ("--output-directory", False),
        ("--verify-directory", False),
        ("--output-directory", True),
        ("--verify-directory", True),
    ],
)
def test_cli_chooses_explicit_complete_or_portable_mode(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], mode: str, *, portable: bool
) -> None:
    paths = (tmp_path / "SHA256SUMS",)
    with (
        patch.object(delivery, "build", return_value=paths) as build,
        patch.object(delivery, "verify", return_value=paths) as verify,
        patch.object(
            qualify_release, "qualify_release", return_value=paths
        ) as portable_build,
        patch.object(
            qualify_release, "verify_release_directory", return_value=paths
        ) as portable_verify,
    ):
        assert (
            delivery.main([
                mode,
                str(tmp_path),
                *(["--portable-only"] if portable else []),
            ])
            == 0
        )
    expected = (
        (portable_verify if mode == "--verify-directory" else portable_build)
        if portable
        else (verify if mode == "--verify-directory" else build)
    )
    expected.assert_called_once_with(tmp_path)
    assert capsys.readouterr().out == str(paths[0]) + "\n"


@pytest.mark.parametrize(
    "name", ["remove-eml-attachments.pyz", "../escape", "..", ".", "", "a\\b"]
)
def test_portable_verifier_refuses_invalid_additional_names(
    tmp_path: Path, name: str
) -> None:
    with pytest.raises(
        ReleaseQualificationError, match=r"^Invalid additional release artifact names$"
    ):
        qualify_release.verify_release_directory(tmp_path, additional_artifacts=(name,))


def test_delivery_retains_build_and_cleanup_errors(tmp_path: Path) -> None:
    output = tmp_path / "release"
    original = RuntimeError("Build failed")
    cleanup = OSError("Cleanup failed")
    with (
        patch.object(sys, "platform", "darwin"),
        patch.object(delivery, "_complete", return_value=()),
        patch.object(delivery, "_copy_verified", side_effect=original),
        patch.object(shutil, "rmtree", side_effect=cleanup),
        pytest.raises(BaseExceptionGroup) as group,
    ):
        delivery._publish(output, tmp_path, ())
    assert group.value.exceptions == (original, cleanup)
    assert (
        group.value.message == "Release delivery failed and staging cleanup also failed"
    )


def test_delivery_module_cli_help_is_available_without_native_tools() -> None:
    result = subprocess.run(
        [sys.executable, "-B", "-m", "tools.release_delivery", "--help"],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert delivery.__doc__ in result.stdout
    assert " ".join(result.stdout.split()).endswith(
        "--portable-only host-local CLI qualification only; "
        "never accepted by the publisher"
    )
    assert "--portable-only" in result.stdout
    assert "--verify-directory" in result.stdout


def test_failed_publication_copy_cleans_reserved_stage(tmp_path: Path) -> None:
    output = tmp_path / "release"
    with (
        patch.object(
            delivery, "_copy_verified", side_effect=RuntimeError("Copy failed")
        ),
        pytest.raises(RuntimeError, match="Copy failed"),
    ):
        delivery._publish(output, tmp_path, ())
    assert not output.exists()
    assert not list(tmp_path.glob(".release.*"))


def test_nonempty_output_is_refused_before_building(tmp_path: Path) -> None:
    (tmp_path / "keep.txt").write_bytes(b"retain")
    with (
        patch.object(sys, "platform", "darwin"),
        patch.object(delivery, "_complete") as complete,
        pytest.raises(ReleaseQualificationError, match="absent or empty"),
    ):
        delivery.build(tmp_path)
    complete.assert_not_called()
    assert (tmp_path / "keep.txt").read_bytes() == b"retain"


@pytest.mark.parametrize(
    "arguments", [[], ["--verify-directory", ".", "--output-directory", "."]]
)
def test_cli_refuses_absent_or_conflicting_modes(arguments: list[str]) -> None:
    with (
        patch.object(delivery, "build") as build,
        patch.object(delivery, "verify") as verify,
        pytest.raises(SystemExit) as error,
    ):
        delivery.main(arguments)
    assert error.value.code == 2
    build.assert_not_called()
    verify.assert_not_called()


def test_direct_cli_help_describes_complete_and_portable_scope(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with pytest.raises(SystemExit) as error:
        delivery.main(["--help"])
    assert error.value.code == 0
    output = capsys.readouterr().out
    assert delivery.__doc__ in output
    assert " ".join(output.split()).endswith(
        "--portable-only host-local CLI qualification only; "
        "never accepted by the publisher"
    )

"""Pure assurance policy primitives have exact portable behavior."""

from __future__ import annotations

import argparse
import io
import tarfile
import zipfile
from typing import TYPE_CHECKING

import pytest
from tools import (
    archive_reproducibility_policy,
    archive_surface_policy,
    repository_path_policy,
    task_test_commands,
    task_timeout,
)

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("value", ["1", "0.25", "1e3"])
def test_positive_timeout_accepts_only_finite_positive_numbers(value: str) -> None:
    assert task_timeout.positive_timeout(value) > 0


@pytest.mark.parametrize("value", ["zero", "0", "-1", "inf", "nan"])
def test_positive_timeout_rejects_invalid_numbers(value: str) -> None:
    with pytest.raises(argparse.ArgumentTypeError):
        task_timeout.positive_timeout(value)


def test_pytest_commands_are_strict_and_boolean_only(tmp_path: Path) -> None:
    direct = task_test_commands.pytest_command(
        "python", tmp_path / "report.xml", coverage=False
    )
    covered = task_test_commands.pytest_command(
        "python", tmp_path / "report.xml", coverage=True
    )
    assert direct[:6] == ("python", "-X", "dev", "-W", "error", "-m")
    assert "pytest" in direct
    assert covered[6:10] == ("coverage", "run", "-m", "pytest")
    with pytest.raises(TypeError):
        task_test_commands._pytest_module(  # ruff: ignore[private-member-access] - direct strict-type contract.
            1
        )


def test_repository_path_policy_reports_each_component_and_collision() -> None:
    assert repository_path_policy.audit_paths(("src/public.py",)) == ()
    issues = repository_path_policy.audit_paths((
        "CON/file.py",
        "name./file.py",
        "same/File.py",
        "same/file.py",
    ))
    messages = {issue.message for issue in issues}
    assert "path component uses a Windows reserved device name" in messages
    assert "path component ends with a Windows-ignored space or dot" in messages
    assert any("collides cross-platform" in message for message in messages)


@pytest.mark.parametrize(
    "name",
    ["/absolute", "a/../b", "a//b", "C:drive", "a\\b", "nul.txt"],
)
def test_archive_surface_rejects_unsafe_or_portability_ambiguous_names(
    name: str,
) -> None:
    with pytest.raises(archive_surface_policy.DistributionArchiveError):
        archive_surface_policy.safe_parts(name, directory=False)


def test_archive_surface_set_directories_and_stream_comparison(tmp_path: Path) -> None:
    assert archive_surface_policy.safe_parts("package/module.py", directory=False) == (
        "package",
        "module.py",
    )
    assert archive_surface_policy.safe_parts("package/", directory=True) == ("package",)
    assert archive_surface_policy.allowed_directories({"a/b/c.txt", "a/d.txt"}) == {
        "a",
        "a/b",
    }
    source = tmp_path / "source.bin"
    source.write_bytes(b"same")
    assert archive_surface_policy.same_content(io.BytesIO(b"same"), source, 2)
    assert not archive_surface_policy.same_content(io.BytesIO(b"different"), source, 2)
    with pytest.raises(archive_surface_policy.DistributionArchiveError):
        archive_surface_policy.require_exact_set("wheel", {"a"}, {"b"})


def test_archive_reproducibility_metadata_contracts() -> None:
    tar_member = tarfile.TarInfo("source.py")
    tar_member.mtime = archive_reproducibility_policy.SOURCE_TIMESTAMP
    tar_member.mode = archive_reproducibility_policy.SOURCE_MODE
    tar_member.uid = tar_member.gid = 0
    tar_member.uname = tar_member.gname = ""
    assert archive_reproducibility_policy.tar_member_normalized(tar_member)
    tar_member.mtime += 1
    assert not archive_reproducibility_policy.tar_member_normalized(tar_member)

    source = zipfile.ZipInfo("source.py")
    source.create_system = archive_reproducibility_policy.UNIX_ZIP_SYSTEM
    source.external_attr = archive_reproducibility_policy.WHEEL_SOURCE_MODE << 16
    source.date_time = archive_reproducibility_policy.ZIP_TIMESTAMP
    assert archive_reproducibility_policy.regular_wheel_member(source)
    assert archive_reproducibility_policy.wheel_member_normalized(
        source, generated=False
    )
    generated = zipfile.ZipInfo("metadata")
    generated.create_system = archive_reproducibility_policy.UNIX_ZIP_SYSTEM
    generated.external_attr = archive_reproducibility_policy.WHEEL_GENERATED_MODE << 16
    generated.date_time = archive_reproducibility_policy.ZIP_TIMESTAMP
    assert archive_reproducibility_policy.wheel_member_normalized(
        generated, generated=True
    )
    generated.external_attr = 0
    assert not archive_reproducibility_policy.wheel_member_normalized(
        generated, generated=True
    )

"""Observation publication saves copies while retaining privacy and rollback."""

from __future__ import annotations

import errno
import json
import os
import shutil
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest
from tools import finalize_hypothesis_artifacts as artifacts
from tools import hypothesis_publication


@pytest.mark.parametrize("ending", ["{invalid\n", '{"detail":"/unknown/private"}\n'])
def test_late_rejection_keeps_original_and_removes_partial_sanitized_output(
    tmp_path: Path, ending: str
) -> None:
    observed = tmp_path / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    path = observed / "records.jsonl"
    original = '{"arguments":{"value":"private generated data"}}\n' + ending
    path.write_text(original, encoding="utf-8")
    publication = tmp_path / "publication"
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "previous.jsonl").write_text("{}\n", encoding="utf-8")
    with pytest.raises(artifacts.HypothesisArtifactError):
        hypothesis_publication.publish_observations(tmp_path, publication)
    assert path.read_text(encoding="utf-8") == original
    assert sorted(child.name for child in observed.iterdir()) == [path.name]
    assert (destination / "previous.jsonl").read_text(encoding="utf-8") == "{}\n"
    assert sorted(child.name for child in destination.parent.iterdir()) == ["observed"]


def test_streaming_reads_lines_and_preserves_unicode_json_string_content(
    tmp_path: Path,
) -> None:
    observed = tmp_path / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    path = observed / "records.jsonl"
    records = [{"detail": "first\u0085second"}, {"status": "passed"}]
    path.write_text(
        "\r\n".join(json.dumps(item, ensure_ascii=False) for item in records),
        encoding="utf-8",
        newline="",
    )
    with patch.object(Path, "read_text", side_effect=AssertionError("whole-file read")):
        assert artifacts.finalize(tmp_path, observations=True) == (1, 2)
    assert [
        json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()
    ] == records


def test_same_filesystem_publication_consumes_without_copying(tmp_path: Path) -> None:
    source = tmp_path / "source"
    observed = source / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    (observed / "records.jsonl").write_text("{}\n", encoding="utf-8")
    publication = tmp_path / "public"
    publication.mkdir()
    with patch.object(shutil, "copyfile", side_effect=AssertionError("extra copy")):
        assert hypothesis_publication.publish_observations(source, publication) == (
            1,
            1,
        )
    assert not observed.exists()
    assert (publication / ".hypothesis/observed/records.jsonl").read_text(
        encoding="utf-8"
    ) == "{}\n"


@pytest.mark.parametrize("copy_fails", [False, True])
def test_cross_device_staging_handles_copy_success_and_failure(
    tmp_path: Path, *, copy_fails: bool
) -> None:
    source = tmp_path / "source"
    observed = source / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    (observed / "records.jsonl").write_text("{}\n", encoding="utf-8")
    publication = tmp_path / "public"
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "previous.jsonl").write_text("{}\n", encoding="utf-8")
    real_rename = os.rename
    real_copy = shutil.copyfile

    def rename(path: Path, target: Path) -> None:
        if path == observed:
            raise OSError(errno.EXDEV, "different devices")
        real_rename(path, target)

    def copy(path: str, target: str) -> str:
        if copy_fails:
            message = "copy failed"
            raise OSError(message)
        return real_copy(path, target)

    with (
        patch.object(os, "rename", side_effect=rename),
        patch.object(shutil, "copyfile", side_effect=copy),
    ):
        if copy_fails:
            with pytest.raises(hypothesis_publication.HypothesisPublicationError):
                hypothesis_publication.publish_observations(source, publication)
            assert observed.exists()
            assert (destination / "previous.jsonl").is_file()
        else:
            assert hypothesis_publication.publish_observations(source, publication) == (
                1,
                1,
            )
            assert not observed.exists()
            assert (destination / "records.jsonl").read_text(encoding="utf-8") == "{}\n"
    assert sorted(path.name for path in destination.parent.iterdir()) == ["observed"]


def test_rejected_stream_closes_staging_before_unlink(tmp_path: Path) -> None:
    observed = tmp_path / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    source = observed / "records.jsonl"
    original = "{}\n{invalid\n"
    source.write_text(original, encoding="utf-8")
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=tmp_path, delete=False
    ) as output:
        staging = Path(output.name)
        real_unlink = Path.unlink

        def unlink(path: Path, *, missing_ok: bool = False) -> None:
            if path == staging and not output.closed:
                message = "Windows sharing violation: staging output is still open"
                raise PermissionError(message)
            real_unlink(path, missing_ok=missing_ok)

        try:
            with (
                patch.object(tempfile, "NamedTemporaryFile", return_value=output),
                patch.object(Path, "unlink", autospec=True, side_effect=unlink),
                pytest.raises(artifacts.HypothesisArtifactError),
            ):
                artifacts.finalize(tmp_path, observations=True)
            assert output.closed
            assert not staging.exists()
            assert source.read_text(encoding="utf-8") == original
        finally:
            output.close()
            staging.unlink(missing_ok=True)


def test_staging_write_error_closes_source_despite_retained_traceback(
    tmp_path: Path,
) -> None:
    observed = tmp_path / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    source = observed / "records.jsonl"
    original = '{"status":"passed"}\n{}\n'
    source.write_text(original, encoding="utf-8")
    publication = tmp_path / "publication"
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "previous.jsonl").write_text("{}\n", encoding="utf-8")
    with (
        source.open(encoding="utf-8") as input_stream,
        tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=tmp_path, delete=False
        ) as output,
    ):
        staging = Path(output.name)
        try:
            with (
                patch.object(Path, "open", return_value=input_stream),
                patch.object(tempfile, "NamedTemporaryFile", return_value=output),
                patch.object(output, "write", side_effect=OSError("disk write failed")),
                pytest.raises(artifacts.HypothesisArtifactError) as raised,
            ):
                hypothesis_publication.publish_observations(tmp_path, publication)
            assert raised.value.__traceback__ is not None
            assert isinstance(raised.value.__cause__, OSError)
            assert input_stream.closed
            assert output.closed
            assert not staging.exists()
        finally:
            output.close()
            staging.unlink(missing_ok=True)
    assert source.read_text(encoding="utf-8") == original
    assert (destination / "previous.jsonl").read_text(encoding="utf-8") == "{}\n"
    assert sorted(child.name for child in observed.iterdir()) == [source.name]

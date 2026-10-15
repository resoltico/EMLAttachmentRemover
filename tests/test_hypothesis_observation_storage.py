"""Observation publication saves copies while retaining privacy and rollback."""

from __future__ import annotations

import errno
import json
import os
import shutil
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
    path.write_text(original)
    publication = tmp_path / "publication"
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "previous.jsonl").write_text("{}\n")
    with pytest.raises(artifacts.HypothesisArtifactError):
        hypothesis_publication.publish_observations(tmp_path, publication)
    assert path.read_text() == original
    assert sorted(child.name for child in observed.iterdir()) == [path.name]
    assert (destination / "previous.jsonl").read_text() == "{}\n"
    assert sorted(child.name for child in destination.parent.iterdir()) == ["observed"]


def test_streaming_reads_lines_and_preserves_unicode_json_string_content(
    tmp_path: Path,
) -> None:
    observed = tmp_path / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    path = observed / "records.jsonl"
    records = [{"detail": "first\u0085second"}, {"status": "passed"}]
    path.write_text(
        "\r\n".join(json.dumps(item, ensure_ascii=False) for item in records)
    )
    with patch.object(Path, "read_text", side_effect=AssertionError("whole-file read")):
        assert artifacts.finalize(tmp_path, observations=True) == (1, 2)
    assert [json.loads(line) for line in path.read_text().splitlines()] == records


def test_same_filesystem_publication_consumes_without_copying(tmp_path: Path) -> None:
    source = tmp_path / "source"
    observed = source / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    (observed / "records.jsonl").write_text("{}\n")
    publication = tmp_path / "public"
    publication.mkdir()
    with patch.object(shutil, "copyfile", side_effect=AssertionError("extra copy")):
        assert hypothesis_publication.publish_observations(source, publication) == (
            1,
            1,
        )
    assert not observed.exists()
    assert (publication / ".hypothesis/observed/records.jsonl").read_text() == "{}\n"


@pytest.mark.parametrize("copy_fails", [False, True])
def test_cross_device_staging_handles_copy_success_and_failure(
    tmp_path: Path, *, copy_fails: bool
) -> None:
    source = tmp_path / "source"
    observed = source / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    (observed / "records.jsonl").write_text("{}\n")
    publication = tmp_path / "public"
    destination = publication / ".hypothesis" / "observed"
    destination.mkdir(parents=True)
    (destination / "previous.jsonl").write_text("{}\n")
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
            assert (destination / "records.jsonl").read_text() == "{}\n"
    assert sorted(path.name for path in destination.parent.iterdir()) == ["observed"]

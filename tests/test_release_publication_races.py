"""Exercise release publication races and immutable read-back boundaries."""

from __future__ import annotations

import re
from typing import TYPE_CHECKING

import pytest
from tools.changelog import ReleaseError
from tools.release_publication import Artifact, Release, publish_release

from tests.test_release_publication import FakeGitHub

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def desired(tmp_path: Path) -> Release:
    paths = [tmp_path / "application.pyz", tmp_path / "SHA256SUMS"]
    for index, path in enumerate(paths):
        path.write_bytes(f"Synthetic qualified artifact {index}".encode())
    return Release(
        "v1.2.3",
        "a" * 40,
        "Canonical changelog text\n",
        tuple(map(Artifact.inspect, paths)),
    )


def test_asset_added_between_reads_is_not_uploaded_twice(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    original = api.assets
    reads = 0

    def assets(release_id: int) -> list[dict[str, object]]:
        nonlocal reads
        reads += 1
        if reads == 2:
            item = desired.artifacts[0]
            api.files.append({
                "name": item.path.name,
                "state": "uploaded",
                "size": item.size,
                "digest": item.digest,
            })
        return original(release_id)

    monkeypatch.setattr(api, "assets", assets)
    publish_release(api, desired)
    assert api.writes == ["create", "upload", "publish"]


def test_tag_change_after_upload_prevents_publication(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    original = api.upload

    def upload(tag: str, artifact: Artifact) -> None:
        original(tag, artifact)
        api.target = "b" * 40

    monkeypatch.setattr(api, "upload", upload)
    with pytest.raises(ReleaseError, match="tag target"):
        publish_release(api, desired)
    assert "publish" not in api.writes


def test_final_readback_rejects_asset_loss_after_publication(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    original = api.publish

    def publish(release_id: int) -> None:
        original(release_id)
        api.files.pop()

    monkeypatch.setattr(api, "publish", publish)
    with pytest.raises(
        ReleaseError, match="Published release is missing expected artifacts"
    ):
        publish_release(api, desired)


def test_draft_with_unacknowledged_upload_is_not_published(desired: Release) -> None:
    api = FakeGitHub(desired)
    api.upload = lambda _tag, _artifact: None  # type: ignore[assignment]
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Draft is not complete for publication") + "$",
    ):
        publish_release(api, desired)
    assert "publish" not in api.writes


def test_final_readback_requires_a_published_release(desired: Release) -> None:
    api = FakeGitHub(desired)
    api.publish = lambda _release_id: None  # type: ignore[assignment]
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Publication read-back did not match") + "$"
    ):
        publish_release(api, desired)


def test_readback_release_identity_cannot_change(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    original = api.release

    def release(release_id: int) -> dict[str, object]:
        value = original(release_id)
        value["id"] = 2
        return value

    monkeypatch.setattr(api, "release", release)
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Release identity changed") + "$"
    ):
        publish_release(api, desired)


def test_release_creation_must_return_a_draft(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    original = api.create

    def create(release: Release) -> dict[str, object]:
        value = original(release)
        value.update({
            "draft": False,
            "immutable": True,
            "published_at": "2026-09-20T00:00:00Z",
        })
        return value

    monkeypatch.setattr(api, "create", create)
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Release creation did not return a draft") + "$",
    ):
        publish_release(api, desired)


def test_missing_asset_is_never_uploaded_to_a_published_release(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    monkeypatch.setattr(
        "tools.release_publication._state",
        lambda *_arguments: (False, [desired.artifacts[0]]),
    )
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Release was published by another writer") + "$",
    ):
        publish_release(api, desired)
    assert "upload" not in api.writes


def test_artifact_change_after_state_read_prevents_upload(
    desired: Release, monkeypatch: pytest.MonkeyPatch
) -> None:
    api = FakeGitHub(desired)
    reads = 0

    def state(*_arguments: object) -> tuple[bool, list[Artifact]]:
        nonlocal reads
        reads += 1
        if reads == 2:
            desired.artifacts[0].path.write_bytes(b"changed after state read")
        return True, [desired.artifacts[0]]

    monkeypatch.setattr("tools.release_publication._state", state)
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Local artifact changed") + "$"
    ):
        publish_release(api, desired)
    assert "upload" not in api.writes

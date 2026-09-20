"""Exercise release reconciliation and uncertain writes using an in-memory GitHub."""

from __future__ import annotations

import re
from copy import deepcopy
from typing import TYPE_CHECKING

import pytest
from tools import release_publication as publication
from tools.changelog import ReleaseError
from tools.release_publication import Artifact, Release, publish_release

if TYPE_CHECKING:
    from pathlib import Path

    from tools.release_publication import Record


class FakeGitHub:
    """In-memory remote which can lose an acknowledgement after a write."""

    def __init__(self, desired: Release) -> None:
        """Start with an exact desired release target."""
        self.desired = desired
        self.target = desired.commit
        self.entries: list[Record] = []
        self.files: list[Record] = []
        self.writes: list[str] = []
        self.lose_response = ""
        self.corrupt_after_publish = False

    def _write(self, operation: str) -> None:
        self.writes.append(operation)
        if self.lose_response == operation:
            self.lose_response = ""
            message = "Response lost after successful remote mutation"
            raise ReleaseError(message)

    def tag_commit(self, tag: str) -> str:
        assert tag == self.desired.tag
        return self.target

    def releases(self) -> list[Record]:
        return deepcopy(self.entries)

    def release(self, release_id: int) -> Record:
        assert release_id == 1
        return deepcopy(self.entries[0])

    def assets(self, release_id: int) -> list[Record]:
        assert release_id == 1
        return deepcopy(self.files)

    def create(self, desired: Release) -> Record:
        self.entries.append({
            "id": 1,
            "tag_name": desired.tag,
            "name": desired.title,
            "body": desired.body,
            "prerelease": False,
            "draft": True,
            "target_commitish": desired.commit,
            "immutable": False,
            "published_at": None,
        })
        self._write("create")
        return deepcopy(self.entries[0])

    def upload(self, tag: str, artifact: Artifact) -> None:
        assert tag == self.desired.tag
        self.files.append({
            "name": artifact.path.name,
            "state": "uploaded",
            "size": artifact.size,
            "digest": artifact.digest,
        })
        self._write("upload")

    def publish(self, release_id: int) -> None:
        assert release_id == 1
        self.entries[0].update({
            "draft": False,
            "immutable": True,
            "published_at": "2026-09-20T00:00:00Z",
        })
        if self.corrupt_after_publish:
            self.entries[0]["body"] = "Unexpectedly edited"
        self._write("publish")


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


def test_create_verify_publish_then_idempotent_noop(desired: Release) -> None:
    api = FakeGitHub(desired)
    assert publish_release(api, desired) == 1
    assert api.writes == ["create", "upload", "upload", "publish"]
    assert publish_release(api, desired) == 1
    assert api.writes == ["create", "upload", "upload", "publish"]


@pytest.mark.parametrize("operation", ["create", "upload", "publish"])
def test_lost_write_response_is_reconciled_on_rerun(
    desired: Release,
    operation: str,
) -> None:
    api = FakeGitHub(desired)
    api.lose_response = operation
    with pytest.raises(ReleaseError, match="Response lost"):
        publish_release(api, desired)
    assert publish_release(api, desired) == 1
    assert api.writes.count("create") == 1
    assert api.writes.count("upload") == len(desired.artifacts)
    assert api.writes.count("publish") == 1


def test_matching_partial_draft_uploads_only_missing_assets(desired: Release) -> None:
    api = FakeGitHub(desired)
    api.create(desired)
    api.upload(desired.tag, desired.artifacts[0])
    api.writes.clear()
    publish_release(api, desired)
    assert api.writes == ["upload", "publish"]


@pytest.mark.parametrize(
    ("key", "value", "message"),
    [
        ("tag_name", "v9.9.9", "Release metadata mismatch: tag_name"),
        ("name", "Different title", "Release metadata mismatch: name"),
        ("body", "Different prose", "Release metadata mismatch: body"),
        ("prerelease", True, "Release metadata mismatch: prerelease"),
        ("prerelease", 0, "Release metadata mismatch: prerelease"),
        ("draft", 1, "Invalid release draft flag"),
        ("target_commitish", "main", "Draft target mismatch"),
        ("id", True, "Invalid GitHub release identifier"),
        ("id", 0, "Invalid GitHub release identifier"),
    ],
)
def test_conflicting_draft_is_not_repaired(
    desired: Release,
    key: str,
    value: object,
    message: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    api = FakeGitHub(desired)
    api.create(desired)
    # Change fresh metadata while retaining the original list result.
    original_read = api.release

    def changed(release_id: int) -> Record:
        result = original_read(release_id)
        result[key] = value
        return result

    monkeypatch.setattr(api, "release", changed)
    api.writes.clear()
    with pytest.raises(ReleaseError, match="^" + message + "$"):
        publish_release(api, desired)
    assert not api.writes


@pytest.mark.parametrize(
    ("key", "value", "message"),
    [
        ("name", "unexpected.exe", "Unexpected or duplicate remote asset"),
        ("name", 1, "Invalid remote asset name"),
        ("state", "starter", "Incomplete remote asset: application.pyz"),
        ("size", True, "Invalid remote asset size: application.pyz"),
        ("size", -1, "Invalid remote asset size: application.pyz"),
        ("digest", None, "Remote asset digest mismatch: application.pyz"),
        (
            "digest",
            "sha256:" + "0" * 64,
            "Remote asset digest mismatch: application.pyz",
        ),
    ],
)
def test_conflicting_assets_are_not_overwritten(
    desired: Release,
    key: str,
    value: object,
    message: str,
) -> None:
    api = FakeGitHub(desired)
    api.create(desired)
    api.upload(desired.tag, desired.artifacts[0])
    api.files[0][key] = value
    api.writes.clear()
    with pytest.raises(ReleaseError, match="^" + message + "$"):
        publish_release(api, desired)
    assert not api.writes


def test_remote_asset_size_must_match_qualified_artifact(desired: Release) -> None:
    api = FakeGitHub(desired)
    api.create(desired)
    api.upload(desired.tag, desired.artifacts[0])
    api.files[0]["size"] = desired.artifacts[0].size + 1
    api.writes.clear()
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Remote asset size mismatch: application.pyz") + "$",
    ):
        publish_release(api, desired)
    assert not api.writes


def test_zero_byte_verified_asset_is_valid(desired: Release, tmp_path: Path) -> None:
    empty = tmp_path / "empty.txt"
    empty.write_bytes(b"")
    plan = Release(
        desired.tag,
        desired.commit,
        desired.body,
        (Artifact.inspect(empty),),
    )
    api = FakeGitHub(plan)
    assert publish_release(api, plan) == 1


@pytest.mark.parametrize(
    "fault",
    ["missing", "extra", "duplicate", "mutable", "undated", "nonstring-date"],
)
def test_invalid_published_release_is_not_repaired(
    desired: Release,
    fault: str,
) -> None:
    api = FakeGitHub(desired)
    publish_release(api, desired)
    if fault == "missing":
        api.files.pop()
    elif fault in {"extra", "duplicate"}:
        api.files.append(deepcopy(api.files[0]))
        if fault == "extra":
            api.files[-1]["name"] = "other.zip"
    elif fault == "mutable":
        api.entries[0]["immutable"] = False
    elif fault == "undated":
        api.entries[0]["published_at"] = None
    else:
        api.entries[0]["published_at"] = 1
    api.writes.clear()
    messages = {
        "missing": "Published release is missing expected artifacts",
        "extra": "Unexpected or duplicate remote asset",
        "duplicate": "Unexpected or duplicate remote asset",
        "mutable": "Published release is not immutable",
        "undated": "Invalid publication date",
        "nonstring-date": "Invalid publication date",
    }
    with pytest.raises(ReleaseError, match="^" + re.escape(messages[fault]) + "$"):
        publish_release(api, desired)
    assert not api.writes


def test_moved_tag_and_duplicate_drafts_fail_before_writes(desired: Release) -> None:
    api = FakeGitHub(desired)
    api.target = "b" * 40
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Remote tag target mismatch") + "$",
    ):
        publish_release(api, desired)
    assert not api.writes
    api.target = desired.commit
    api.create(desired)
    api.entries.append(deepcopy(api.entries[0]))
    api.writes.clear()
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Multiple releases refer to the same tag") + "$",
    ):
        publish_release(api, desired)
    assert not api.writes


def test_local_mutation_and_final_remote_drift_are_detected(desired: Release) -> None:
    api = FakeGitHub(desired)
    desired.artifacts[0].path.write_bytes(b"Changed after qualification")
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Local artifact changed") + "$"
    ):
        publish_release(api, desired)
    assert not api.writes
    revised = Release(
        desired.tag,
        desired.commit,
        desired.body,
        tuple(Artifact.inspect(item.path) for item in desired.artifacts),
    )
    api = FakeGitHub(revised)
    api.corrupt_after_publish = True
    with pytest.raises(ReleaseError, match="body"):
        publish_release(api, revised)
    assert api.writes[-1] == "publish"


def test_local_roster_and_file_types_are_checked(
    desired: Release,
    tmp_path: Path,
) -> None:
    for artifacts in [(), (desired.artifacts[0], desired.artifacts[0])]:
        plan = Release(desired.tag, desired.commit, desired.body, artifacts)
        api = FakeGitHub(plan)
        message = (
            "No verified release artifacts supplied"
            if not artifacts
            else "Duplicate local artifact names"
        )
        with pytest.raises(
            ReleaseError,
            match="^" + re.escape(message) + "$",
        ):
            publish_release(api, plan)
        assert not api.writes
    with pytest.raises(ReleaseError):
        Artifact.inspect(tmp_path)


@pytest.mark.parametrize(
    ("value", "message"),
    [
        (None, "Expected a GitHub JSON object"),
        ([], "Expected a GitHub JSON object"),
        ({1: "value"}, "Invalid JSON object keys"),
    ],
)
def test_record_rejects_non_object_and_non_string_keys(
    value: object, message: str
) -> None:
    with pytest.raises(ReleaseError, match="^" + re.escape(message) + "$"):
        publication.record(value)


@pytest.mark.parametrize(
    "item",
    [{}, {"id": None}, {"id": True}, {"id": 0}, {"id": -1}, {"id": 1.0}],
)
def test_identifier_accepts_only_positive_plain_integers(item: Record) -> None:
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Invalid GitHub release identifier") + "$"
    ):
        publication.identifier(item)

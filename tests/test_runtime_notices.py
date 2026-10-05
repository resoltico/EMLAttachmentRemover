"""Dependency notices must survive runtime repackaging, not merely its checksum."""

from __future__ import annotations

import hashlib
import json
from typing import TYPE_CHECKING

import pytest
from tools import runtime_notices

if TYPE_CHECKING:
    from pathlib import Path


def test_missing_notices_are_supplied_without_replacing_upstream_text(
    tmp_path: Path,
) -> None:
    licenses = tmp_path / "licenses"
    licenses.mkdir()
    original = licenses / "LICENSE.zstd.txt"
    original.write_bytes(b"upstream notice retained verbatim")
    (tmp_path / "PYTHON.json").write_text(
        json.dumps({
            "license_path": "licenses/LICENSE.zstd.txt",
            "build_info": {
                "extensions": {
                    "zlib": [{"license_paths": ["licenses/LICENSE.zlib-ng.txt"]}]
                }
            },
        })
    )
    runtime_notices.apply(tmp_path)
    assert original.read_bytes() == b"upstream notice retained verbatim"
    assert (licenses / "LICENSE.zlib-ng.txt").read_bytes() == (
        runtime_notices.SUPPLEMENTS / "LICENSE.zlib-ng.txt"
    ).read_bytes()


@pytest.mark.parametrize("name", ["licenses/missing.txt", "../outside", "/absolute"])
def test_missing_or_escaping_references_refuse_runtime(
    tmp_path: Path, name: str
) -> None:
    (tmp_path / "licenses").mkdir()
    (tmp_path / "PYTHON.json").write_text(json.dumps({"license_path": name}))
    with pytest.raises(ValueError, match="declared license notice"):
        runtime_notices.apply(tmp_path)


@pytest.mark.parametrize("kind", ["empty", "symlink"])
def test_a_declared_notice_must_be_an_ordinary_nonempty_file(
    tmp_path: Path, kind: str
) -> None:
    licenses = tmp_path / "licenses"
    licenses.mkdir()
    notice = licenses / "required.txt"
    if kind == "empty":
        notice.touch()
    else:
        target = tmp_path / "original"
        target.write_text("public test notice")
        notice.symlink_to(target)
    (tmp_path / "PYTHON.json").write_text(
        json.dumps({"license_path": "licenses/required.txt"})
    )
    with pytest.raises(ValueError, match="declared license notice"):
        runtime_notices.apply(tmp_path)


@pytest.mark.parametrize(
    "value", [{"license_path": False}, {"license_paths": [1]}, {"license_paths": "x"}]
)
def test_malformed_reference_fields_are_refused(value: object) -> None:
    with pytest.raises(ValueError, match=r"^invalid runtime license references$"):
        runtime_notices.references(value)


def test_supplemental_texts_match_the_reviewed_upstream_notices() -> None:
    """Independent public reference hashes detect edits to required notice text."""
    expected = {
        "LICENSE.zstd.txt": (
            "7055266497633c9025b777c78eb7235af"  # pragma: allowlist secret
            "13922117480ed5c674677adc381c9d8"  # pragma: allowlist secret
        ),
        "LICENSE.zlib-ng.txt": (
            "6c9f0d975b41afaa34d22f55bb8986ce"  # pragma: allowlist secret
            "69e5cb7ad327cb2b28820cd425edf5ee"  # pragma: allowlist secret
        ),
    }
    for name, digest in expected.items():
        assert (
            hashlib.sha256(
                (runtime_notices.SUPPLEMENTS / name).read_bytes()
            ).hexdigest()
            == digest
        )


@pytest.mark.parametrize("name", ["other/required.txt", "licenses/../required.txt"])
def test_existing_notices_cannot_escape_the_declared_license_directory(
    tmp_path: Path,
    name: str,
) -> None:
    (tmp_path / "licenses").mkdir()
    notice = tmp_path / name
    notice.parent.mkdir(parents=True, exist_ok=True)
    notice.write_bytes(b"present but outside the declared license directory")
    (tmp_path / "PYTHON.json").write_text(
        json.dumps({
            "nested": {"extensions": [{"license_path": name}]},
        })
    )
    with pytest.raises(ValueError, match="declared license notice") as caught:
        runtime_notices.apply(tmp_path)
    assert (
        str(caught.value)
        == "runtime is missing a safe, nonempty declared license notice"
    )


def test_notice_references_are_collected_through_dictionary_and_list_children() -> None:
    assert runtime_notices.references({
        "nested": {"license_path": "licenses/dictionary.txt"},
        "extensions": [{"license_paths": ["licenses/list.txt"]}],
    }) == {"licenses/dictionary.txt", "licenses/list.txt"}

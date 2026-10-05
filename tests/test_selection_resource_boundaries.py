"""Selection budgets include equality and directory identities prevent repeat scans."""

from __future__ import annotations

import os
import stat
from pathlib import Path
from types import SimpleNamespace

import pytest

from eml_attachment_remover import selection_collection
from eml_attachment_remover.domain import AppError, ExitCode


def test_literal_tilde_selection_is_not_deduplicated_against_home(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)
    paths = ["~/public-uncreated.eml", str(Path.home() / "public-uncreated.eml")]
    assert selection_collection.collect(paths) == paths


def test_parent_traversal_through_a_symlink_keeps_distinct_sources(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nested = tmp_path / "outside" / "nested"
    nested.mkdir(parents=True)
    (nested.parent / "message.eml").write_bytes(b"outside")
    (tmp_path / "message.eml").write_bytes(b"inside")
    (tmp_path / "link").symlink_to(nested, target_is_directory=True)
    monkeypatch.chdir(tmp_path)
    paths = ["link/../message.eml", "message.eml"]
    assert selection_collection.collect(paths) == paths


def test_explicit_selection_accounts_for_all_native_path_bytes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    paths = [str(tmp_path / "first.eml"), str(tmp_path / "second.eml")]
    for path in paths:
        Path(path).write_bytes(b"retained")
    budget = sum(len(os.fsencode(path)) for path in paths)
    monkeypatch.setattr(
        selection_collection, "MAX_CUMULATIVE_REQUEST_PATH_BYTES", budget
    )
    assert selection_collection.collect(paths) == paths
    assert selection_collection.collect([str(tmp_path)]) == paths
    monkeypatch.setattr(
        selection_collection, "MAX_CUMULATIVE_REQUEST_PATH_BYTES", budget - 1
    )
    with pytest.raises(AppError) as caught:
        selection_collection.collect([str(tmp_path)])
    assert caught.value.code is ExitCode.USAGE
    assert caught.value.message == (
        "The selected file paths exceed the request-size limit. "
        "Choose fewer files or shorter paths. No copies were created."
    )


def test_scanned_entry_budget_counts_non_email_entries_and_includes_equality(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"retained")
    (tmp_path / ".ignored").write_bytes(b"ignored")
    monkeypatch.setattr(selection_collection, "MAX_SCANNED_ENTRIES", 2)
    assert selection_collection.collect([str(tmp_path)]) == [str(source)]
    monkeypatch.setattr(selection_collection, "MAX_SCANNED_ENTRIES", 1)
    with pytest.raises(AppError) as caught:
        selection_collection.collect([str(tmp_path)])
    assert caught.value.code is ExitCode.USAGE
    assert caught.value.message == "folder traversal exceeds its entry limit"


@pytest.mark.parametrize("inode", [0, 199])
def test_directory_identity_deduplicates_without_spending_the_budget(
    inode: int,
) -> None:
    collection = selection_collection.__dict__["_Collection"]()
    snapshot = SimpleNamespace(st_dev=4, st_ino=inode)
    assert collection.enter("first", snapshot) is True
    assert collection.enter("first", snapshot) is False
    assert collection.directory_bytes == len(os.fsencode("first"))
    assert collection.enter("second", snapshot) is (inode == 0)
    assert len(collection.directories) == (2 if inode == 0 else 1)


def test_directory_path_budget_is_cumulative_and_includes_equality(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    collection = selection_collection.__dict__["_Collection"]()
    monkeypatch.setattr(selection_collection, "MAX_CUMULATIVE_REQUEST_PATH_BYTES", 3)
    assert collection.enter("a", SimpleNamespace(st_dev=1, st_ino=1)) is True
    assert collection.enter("bb", SimpleNamespace(st_dev=1, st_ino=2)) is True
    with pytest.raises(AppError) as caught:
        collection.enter("c", SimpleNamespace(st_dev=1, st_ino=3))
    assert caught.value.code is ExitCode.USAGE
    assert (
        caught.value.message == "folder traversal exceeds directory or path-byte limits"
    )


def test_reparse_directory_entries_are_links_even_without_posix_link_mode(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 1024, raising=False)
    link = selection_collection.__dict__["_link"]
    assert link(SimpleNamespace(st_mode=stat.S_IFDIR, st_file_attributes=1024)) is True
    assert link(SimpleNamespace(st_mode=stat.S_IFDIR, st_file_attributes=1)) is False
    assert link(SimpleNamespace(st_mode=stat.S_IFDIR)) is False
    monkeypatch.delattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT")
    assert link(SimpleNamespace(st_mode=stat.S_IFDIR, st_file_attributes=1)) is False

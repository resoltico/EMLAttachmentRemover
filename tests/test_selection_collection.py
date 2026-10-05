"""Folder collection preserves explicit intent and refuses incomplete traversal."""

from __future__ import annotations

import os
from contextlib import contextmanager
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import selection_collection
from eml_attachment_remover.domain import AppError, ExitCode

if TYPE_CHECKING:
    from collections.abc import Generator, Iterator
    from contextlib import AbstractContextManager


def test_nested_folders_are_ordered_deduplicated_and_generated_copies_are_excluded(
    tmp_path: Path,
) -> None:
    """Folder scans omit generated copies; explicit file choices remain valid."""
    root = tmp_path / "emails"
    nested = root / "nested"
    nested.mkdir(parents=True)
    first = root / "a.EML"
    second = nested / "b.eml"
    generated = root / "a.mime-pruned.eml"
    for path in [first, second, generated, root / "note.txt"]:
        path.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    selected = selection_collection.collect([str(root), str(nested), str(first)])
    assert selected == [str(first), str(second)]
    assert selection_collection.collect([str(generated), str(root)]) == [
        str(generated),
        str(first),
        str(second),
    ]


def test_empty_folders_and_missing_explicit_files_have_distinct_outcomes(
    tmp_path: Path,
) -> None:
    """Empty collections fail; explicit file errors remain backend outcomes."""
    with pytest.raises(AppError, match="no EML files") as failure:
        selection_collection.collect([str(tmp_path)])
    assert failure.value.code is ExitCode.USAGE
    missing = tmp_path / "missing.eml"
    assert selection_collection.collect([str(missing)]) == [str(missing)]


def test_directory_links_do_not_create_traversal_loops(tmp_path: Path) -> None:
    """Folder scans skip links; explicit directory links are refused."""
    root = tmp_path / "emails"
    root.mkdir()
    source = root / "message.eml"
    source.write_bytes(b"body")
    link = root / "loop"
    link.symlink_to(root, target_is_directory=True)
    assert selection_collection.collect([str(root)]) == [str(source)]
    with pytest.raises(AppError, match="directory links") as failure:
        selection_collection.collect([str(link)])
    assert failure.value.code is ExitCode.INPUT_ERROR


def test_explicit_file_aliases_reach_backend_collision_validation(
    tmp_path: Path,
) -> None:
    """Deduplication retains distinct aliases for backend identity checks."""
    source = tmp_path / "source.eml"
    source.write_bytes(b"body")
    alias = tmp_path / "alias.eml"
    alias.symlink_to(source)
    assert selection_collection.collect([str(source), str(alias)]) == [
        str(source),
        str(alias),
    ]
    assert selection_collection.collect([str(tmp_path)]) == [str(source)]


@pytest.mark.parametrize(
    ("limit", "value"),
    [
        ("MAX_SCANNED_ENTRIES", 1),
        ("MAX_DIRECTORIES", 1),
        ("MAX_BATCH_ITEMS", 1),
        ("MAX_CUMULATIVE_REQUEST_PATH_BYTES", 1),
    ],
)
def test_collection_resource_limits_refuse_the_whole_selection(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    limit: str,
    value: int,
) -> None:
    """Collection resource limits stop before any processing."""
    nested = tmp_path / "nested"
    nested.mkdir()
    (tmp_path / "first.eml").write_bytes(b"first")
    (nested / "second.eml").write_bytes(b"second")
    monkeypatch.setattr(selection_collection, limit, value)
    with pytest.raises(AppError) as failure:
        selection_collection.collect([str(tmp_path)])
    assert failure.value.code is ExitCode.USAGE
    assert list(tmp_path.rglob("*.mime-pruned.eml")) == []


def test_unreadable_enumeration_never_returns_a_partial_selection(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Enumeration failures cannot silently turn a requested tree into a subset."""
    original = os.scandir
    nested = tmp_path / "unreadable"
    nested.mkdir()
    source = tmp_path / "first.eml"
    source.write_bytes(b"first")

    def denied(path: str) -> AbstractContextManager[Iterator[os.DirEntry[str]]]:
        if Path(path) == nested:
            message = "controlled enumeration refusal"
            raise PermissionError(message)
        return original(path)

    monkeypatch.setattr(os, "scandir", denied)
    with pytest.raises(AppError, match="enumerate selected folder") as failure:
        selection_collection.collect([str(tmp_path)])
    assert failure.value.code is ExitCode.INPUT_ERROR
    assert source.read_bytes() == b"first"
    assert not source.with_suffix(".mime-pruned.eml").exists()


def test_directory_replacement_during_scan_refuses_collected_entries(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Observed directory identity changes stop before any MIME processing."""
    root = tmp_path / "selected"
    root.mkdir()
    source = root / "message.eml"
    source.write_bytes(b"original body")
    retained = tmp_path / "retained"
    original = os.scandir

    @contextmanager
    def replaced(path: str) -> Generator[Iterator[os.DirEntry[str]]]:
        with original(path) as stream:
            entries = list(stream)
        root.rename(retained)
        root.mkdir()
        (root / "message.eml").write_bytes(b"replacement body")
        yield iter(entries)

    monkeypatch.setattr(os, "scandir", replaced)
    with pytest.raises(AppError, match="changed during collection") as failure:
        selection_collection.collect([str(root)])
    assert failure.value.code is ExitCode.INPUT_ERROR
    assert (retained / "message.eml").read_bytes() == b"original body"
    assert not (root / "message.mime-pruned.eml").exists()

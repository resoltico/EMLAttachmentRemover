"""Bounded, deterministic folder selection shared by pipe-based frontends."""

from __future__ import annotations

import os
import stat
from dataclasses import dataclass, field
from pathlib import Path
from typing import Final

from .batch import MAX_BATCH_ITEMS, MAX_CUMULATIVE_REQUEST_PATH_BYTES
from .cancellation import checkpoint
from .destination_names import SUFFIX
from .domain import AppError, ExitCode

MAX_SCANNED_ENTRIES: Final = 100_000
MAX_DIRECTORIES: Final = 4_096


def _key(path: str) -> str:
    """Anchor a pathname without interpreting shorthand or collapsing traversal.

    Returns:
        An absolute spelling preserving case and parent traversal.

    """
    return str(Path(path).absolute())


def _link(snapshot: os.stat_result) -> bool:
    """Recognize ordinary symlinks and Windows reparse entries.

    Returns:
        Whether traversal must avoid this directory entry.

    """
    reparse = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    attributes = getattr(snapshot, "st_file_attributes", 0)
    return stat.S_ISLNK(snapshot.st_mode) or bool(attributes & reparse)


@dataclass
class _Collection:
    """Own selection order, unique pathname membership and traversal budgets."""

    paths: list[str] = field(default_factory=list)
    path_keys: set[str] = field(default_factory=set)
    directories: set[tuple[int, int] | str] = field(default_factory=set)
    source_bytes: int = 0
    directory_bytes: int = 0
    entries: int = 0

    def add(self, path: str) -> None:
        """Add one unique source while preserving aliases for backend validation.

        Raises:
            AppError: If the collected selection exceeds request resource limits.

        """
        key = _key(path)
        if key in self.path_keys:
            return
        self.source_bytes += len(os.fsencode(path))
        if len(self.paths) >= MAX_BATCH_ITEMS:
            raise AppError(
                ExitCode.USAGE,
                f"The selection contains more than {MAX_BATCH_ITEMS:,} email files. "
                "Choose a subfolder or select fewer files. No copies were created.",
                phase="selection",
            )
        if self.source_bytes > MAX_CUMULATIVE_REQUEST_PATH_BYTES:
            raise AppError(
                ExitCode.USAGE,
                "The selected file paths exceed the request-size limit. "
                "Choose fewer files or shorter paths. No copies were created.",
                phase="selection",
            )
        self.path_keys.add(key)
        self.paths.append(path)

    def enter(self, path: str, snapshot: os.stat_result) -> bool:
        """Claim an observed directory identity, keeping traversal bounded.

        Returns:
            Whether this directory has not already been enumerated.

        Raises:
            AppError: If directory traversal exceeds its resource limits.

        """
        identity: tuple[int, int] | str = (
            (snapshot.st_dev, snapshot.st_ino) if snapshot.st_ino else _key(path)
        )
        if identity in self.directories:
            return False
        self.directory_bytes += len(os.fsencode(path))
        if (
            len(self.directories) >= MAX_DIRECTORIES
            or self.directory_bytes > MAX_CUMULATIVE_REQUEST_PATH_BYTES
        ):
            raise AppError(
                ExitCode.USAGE, "folder traversal exceeds directory or path-byte limits"
            )
        self.directories.add(identity)
        return True

    def scan(self, path: str) -> list[os.DirEntry[str]]:
        """Enumerate one bounded directory in native filename order.

        Returns:
            Its directory entries, without opening message contents.

        Raises:
            AppError: If enumeration fails or exceeds the entry budget.

        """
        entries: list[os.DirEntry[str]] = []
        with os.scandir(path) as stream:
            for entry in stream:
                checkpoint()
                self.entries += 1
                if self.entries > MAX_SCANNED_ENTRIES:
                    raise AppError(
                        ExitCode.USAGE, "folder traversal exceeds its entry limit"
                    )
                if not entry.name.startswith("."):
                    entries.append(entry)
        return sorted(entries, key=lambda entry: os.fsencode(entry.name))


def _folder(path: str, collection: _Collection) -> list[str]:
    """Collect one observed directory, refusing links and detectable identity changes.

    Returns:
        Child directories for subsequent bounded traversal.

    Raises:
        AppError: If a directory changed or enumeration cannot be completed.

    """
    before = Path(path).stat(follow_symlinks=False)
    if _link(before) or not stat.S_ISDIR(before.st_mode):
        raise AppError(ExitCode.INPUT_ERROR, "selected folder changed or became a link")
    if not collection.enter(path, before):
        return []
    children: list[str] = []
    for entry in collection.scan(path):
        checkpoint()
        snapshot = entry.stat(follow_symlinks=False)
        if _link(snapshot):
            continue
        if stat.S_ISDIR(snapshot.st_mode):
            children.append(entry.path)
        elif (
            stat.S_ISREG(snapshot.st_mode)
            and entry.name.lower().endswith(".eml")
            and not entry.name.lower().endswith(SUFFIX)
        ):
            collection.add(entry.path)
    after = Path(path).stat(follow_symlinks=False)
    if (
        _link(after)
        or not stat.S_ISDIR(after.st_mode)
        or (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino)
    ):
        raise AppError(
            ExitCode.INPUT_ERROR, "selected folder changed during collection"
        )
    return children


def collect(selected: list[str]) -> list[str]:
    """Expand folders before processing, preserving explicitly selected file intent.

    Returns:
        Unique sources in selection order and deterministic folder traversal order.

    Raises:
        AppError: If enumeration fails, finds no EML files or exceeds its budgets.

    """
    collection = _Collection()
    for original in selected:
        checkpoint()
        path = str(Path(original).expanduser().absolute())
        try:
            snapshot = Path(path).stat(follow_symlinks=False)
        except OSError:
            # Missing/inaccessible explicit files retain per-item backend errors.
            collection.add(original)
            continue
        if _link(snapshot) and Path(path).is_dir():
            raise AppError(ExitCode.INPUT_ERROR, "directory links are not collected")
        if not stat.S_ISDIR(snapshot.st_mode):
            collection.add(original)
            continue
        pending = [path]
        try:
            while pending:
                checkpoint()
                pending.extend(reversed(_folder(pending.pop(), collection)))
        except OSError as error:
            raise AppError(
                ExitCode.INPUT_ERROR, "could not enumerate selected folder"
            ) from error
    if not collection.paths:
        raise AppError(ExitCode.USAGE, "selection contains no EML files")
    return collection.paths

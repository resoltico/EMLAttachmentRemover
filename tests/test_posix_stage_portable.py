"""Portable simulation of POSIX stage ownership, publication and cleanup faults."""

from __future__ import annotations

import errno
import stat
from dataclasses import dataclass

import pytest

from eml_attachment_remover import native_posix_stage
from eml_attachment_remover.domain import AppError, BoundDirectory, ExitCode

_PARENT = 11
_STAGE = 22
_FILE = 33
_NAME = b"private-stage"


@dataclass(frozen=True)
class _Info:
    """Only metadata fields consumed by the POSIX stage owner."""

    st_mode: int
    st_dev: int = 1
    st_ino: int = 2
    st_uid: int = 1000


class _FakePOSIX:
    """Simulate descriptor-relative operations without relying on the host OS."""

    def __init__(self) -> None:
        self.outer = False
        self.payload = False
        self.mode = 0o700
        self.held_kind = stat.S_IFDIR
        self.entry_kind = stat.S_IFDIR
        self.file_kind = stat.S_IFREG
        self.file_entry_kind = stat.S_IFREG
        self.held_inode = 2
        self.entry_inode = 2
        self.file_inode = 3
        self.file_entry_inode = 3
        self.owner = 1000
        self.uid = 1000
        self.fail_directory_open = False
        self.fail_file_open = False
        self.fail_mkdir = False
        self.fail_unlink = False
        self.fail_rmdir = False
        self.fail_close = False
        self.closed: list[int] = []

    def geteuid(self) -> int:
        """Return the synthetic effective user identity.

        Returns:
            The user ID owning the private directory.

        """
        return self.uid

    def fstat(self, descriptor: int) -> _Info:
        """Describe one held descriptor independently of its external name.

        Returns:
            The identity facts for a live synthetic handle.

        """
        if descriptor == _STAGE:
            return _Info(
                self.held_kind | self.mode, st_ino=self.held_inode, st_uid=self.owner
            )
        if descriptor == _FILE:
            return _Info(self.file_kind | 0o600, st_ino=self.file_inode)
        raise OSError(errno.EBADF, "unexpected descriptor")

    def stat(self, name: bytes, *, dir_fd: int, follow_symlinks: bool) -> _Info:
        """Inspect one synthetic directory entry without following symlinks.

        Returns:
            Its stat-like identity and file type metadata.

        """
        assert not follow_symlinks
        if dir_fd == _PARENT and name == _NAME and self.outer:
            return _Info(
                self.entry_kind | self.mode, st_ino=self.entry_inode, st_uid=self.owner
            )
        if dir_fd == _STAGE and name == b"candidate" and self.payload:
            return _Info(self.file_entry_kind | 0o600, st_ino=self.file_entry_inode)
        raise FileNotFoundError(errno.ENOENT, "no such synthetic entry")

    def mkdir(self, name: bytes, mode: int, *, dir_fd: int) -> None:
        """Create exactly one owner-private parent-relative staging directory."""
        assert name == _NAME and mode == 0o700 and dir_fd == _PARENT
        if self.outer or self.fail_mkdir:
            raise FileExistsError(errno.EEXIST, "synthetic directory exists")
        self.outer = True

    def open(self, name: bytes, _flags: int, mode: int = 0o777, *, dir_fd: int) -> int:
        """Open the protected directory or create its candidate file.

        Returns:
            The simulated owned directory or candidate descriptor.

        """
        if dir_fd == _PARENT and name == _NAME:
            if self.fail_directory_open:
                raise OSError(errno.EIO, "directory open failed")
            assert self.outer
            return _STAGE
        if dir_fd == _STAGE and name == b"candidate":
            assert mode == 0o600
            if self.fail_file_open:
                raise OSError(errno.EIO, "candidate open failed")
            assert not self.payload
            self.payload = True
            return _FILE
        raise OSError(errno.EBADF, "invalid synthetic open")

    def close(self, descriptor: int) -> None:
        """Record releases, optionally reporting a post-close failure."""
        self.closed.append(descriptor)
        if self.fail_close:
            raise OSError(errno.EIO, "simulated close failure")

    def unlink(self, name: bytes, *, dir_fd: int) -> None:
        """Remove the inner candidate without touching another parent."""
        assert name == b"candidate" and dir_fd == _STAGE
        if self.fail_unlink:
            raise OSError(errno.EIO, "unlink failed")
        if not self.payload:
            raise FileNotFoundError(errno.ENOENT, "already renamed")
        self.payload = False

    def rmdir(self, name: bytes, *, dir_fd: int) -> None:
        """Remove only the empty, owner-identified outer directory."""
        assert name == _NAME and dir_fd == _PARENT
        if self.fail_rmdir:
            raise OSError(errno.EIO, "rmdir failed")
        if self.payload:
            raise OSError(errno.ENOTEMPTY, "candidate remains")
        self.outer = False


def _bound(monkeypatch: pytest.MonkeyPatch) -> tuple[_FakePOSIX, BoundDirectory]:
    """Install isolated POSIX syscall fakes and return a fresh live parent.

    Returns:
        The fake kernel state and its bound parent descriptor.

    """
    kernel = _FakePOSIX()
    monkeypatch.setattr(native_posix_stage, "os", kernel)
    monkeypatch.setattr(
        native_posix_stage.native_posix_acl, "restrict", lambda _fd: None
    )
    return kernel, BoundDirectory(_PARENT, windows=False)


def test_portable_private_stage_success_and_named_cleanup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The published candidate is selected through a held source directory."""
    kernel, parent = _bound(monkeypatch)
    descriptor = native_posix_stage.create(parent, _NAME)
    calls: list[tuple[int, int, bytes, bytes]] = []

    def publish(source: int, target: int, source_name: bytes, output: bytes) -> bool:
        """Capture the protected hard-link source and published destination.

        Returns:
            Whether the hard-link source remains to be unlinked.

        """
        calls.append((source, target, source_name, output))
        return True

    monkeypatch.setattr(native_posix_stage, "publish_no_replace", publish)
    assert descriptor == _FILE
    assert native_posix_stage.publish(parent, descriptor, _NAME, b"out") is True
    assert calls == [(_STAGE, _PARENT, b"candidate", b"out")]
    native_posix_stage.discard(parent, descriptor, _NAME)
    assert not kernel.payload
    assert not kernel.outer


@pytest.mark.parametrize(
    "fault",
    ["missing", "entry-type", "held-type", "inode", "owner", "permissions"],
)
def test_portable_private_stage_rejects_changed_outer_identity(
    monkeypatch: pytest.MonkeyPatch, fault: str
) -> None:
    """Every owner-private namespace guard fails closed."""
    kernel, parent = _bound(monkeypatch)
    stage = native_posix_stage.create(parent, _NAME)
    if fault == "missing":
        kernel.outer = False
    elif fault == "entry-type":
        kernel.entry_kind = stat.S_IFREG
    elif fault == "held-type":
        kernel.held_kind = stat.S_IFREG
    elif fault == "inode":
        kernel.entry_inode = 99
    elif fault == "owner":
        kernel.owner += 1
    else:
        kernel.mode = 0o755
    with pytest.raises(AppError, match="private stage directory was renamed"):
        native_posix_stage.discard(parent, stage, _NAME)


def test_portable_stage_rejects_unowned_or_unavailable_namespace(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unowned bindings cannot acquire or publish a fabricated file."""
    kernel, parent = _bound(monkeypatch)
    parent.private_stage_directory_fd = _STAGE
    with pytest.raises(AppError, match="private stage already owned"):
        native_posix_stage.create(parent, _NAME)
    parent.private_stage_directory_fd = None
    with pytest.raises(AppError, match="private stage directory is absent"):
        native_posix_stage.publish(parent, _FILE, _NAME, b"out")
    with pytest.raises(AppError, match="private stage directory is absent"):
        native_posix_stage.discard(parent, _FILE, _NAME)
    kernel.fail_mkdir = True
    with pytest.raises(FileExistsError, match="synthetic directory exists"):
        native_posix_stage.create(parent, _NAME)


@pytest.mark.parametrize("failure_point", ["directory", "candidate", "identity"])
def test_portable_create_releases_every_acquired_owner(
    monkeypatch: pytest.MonkeyPatch, failure_point: str
) -> None:
    """A failed acquisition releases only handles that were actually opened."""
    kernel, parent = _bound(monkeypatch)
    if failure_point == "directory":
        kernel.fail_directory_open = True
    elif failure_point == "candidate":
        kernel.fail_file_open = True
    else:
        kernel.entry_inode = 9
    with pytest.raises(
        (OSError, AppError), match="open failed|private staging directory changed"
    ):
        native_posix_stage.create(parent, _NAME)
    assert parent.private_stage_directory_fd is None
    if failure_point == "directory":
        assert kernel.outer and kernel.closed == []
    elif failure_point == "candidate":
        assert not kernel.outer and kernel.closed == [_STAGE]
    else:
        assert kernel.outer and kernel.closed == [_STAGE]


def test_portable_creation_preserves_independent_cleanup_errors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Acquisition and independent handle cleanup faults retain every cause."""
    kernel, parent = _bound(monkeypatch)
    kernel.fail_file_open = True
    kernel.fail_close = True
    with pytest.raises(BaseExceptionGroup, match="acquisition and cleanup") as failure:
        native_posix_stage.create(parent, _NAME)
    assert len(failure.value.exceptions) == 2
    assert isinstance(failure.value.exceptions[0], OSError)
    assert kernel.closed == [_STAGE]


def test_portable_creation_cleanup_retains_all_native_failures(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Every cleanup step executes even when each independent operation fails."""
    kernel, parent = _bound(monkeypatch)
    kernel.outer = True
    kernel.payload = True
    kernel.fail_close = True
    kernel.fail_unlink = True
    kernel.fail_rmdir = True
    result = native_posix_stage._creation_cleanup(  # ruff: ignore[private-member-access] - direct fault aggregation contract.
        parent, _NAME, _STAGE, _FILE
    )
    assert len(result) == 4
    assert kernel.closed == [_FILE, _STAGE]


@pytest.mark.parametrize("fault", ["held-kind", "entry-kind", "inode"])
def test_portable_publish_rejects_source_substitution(
    monkeypatch: pytest.MonkeyPatch, fault: str
) -> None:
    """A swapped candidate cannot reach the visibility operation."""
    kernel, parent = _bound(monkeypatch)
    candidate = native_posix_stage.create(parent, _NAME)
    if fault == "held-kind":
        kernel.file_kind = stat.S_IFDIR
    elif fault == "entry-kind":
        kernel.file_entry_kind = stat.S_IFDIR
    else:
        kernel.file_entry_inode = 45
    with pytest.raises(AppError, match="private staged file changed"):
        native_posix_stage.publish(parent, candidate, _NAME, b"out")


def test_portable_discard_handles_consumed_rename_and_refuses_faults(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Missing moved candidates are harmless; genuine directory errors propagate."""
    kernel, parent = _bound(monkeypatch)
    candidate = native_posix_stage.create(parent, _NAME)
    kernel.payload = False
    native_posix_stage.discard(parent, candidate, _NAME)
    assert not kernel.outer
    kernel, parent = _bound(monkeypatch)
    candidate = native_posix_stage.create(parent, _NAME)
    kernel.fail_rmdir = True
    with pytest.raises(OSError, match="rmdir failed"):
        native_posix_stage.discard(parent, candidate, _NAME)

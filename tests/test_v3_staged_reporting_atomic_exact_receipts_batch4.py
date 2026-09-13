"""Exact safety receipts for publication primitives and public reporting."""

from __future__ import annotations

import ctypes
import errno
from dataclasses import dataclass
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import atomic_publish, reporting_v3, staged_output
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import bind_destination
from eml_attachment_remover.staged_output import PublishedWithError

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.staged_output import _PublicationState


class _RenameOperation:
    """Observable Darwin rename stand-in with a configured native result."""

    def __init__(self, result: int) -> None:
        self.result = result
        self.argtypes: list[object] | None = None
        self.restype: object | None = None
        self.calls: list[tuple[object, ...]] = []

    def __call__(self, *arguments: object) -> int:
        self.calls.append(arguments)
        return self.result


@dataclass
class _RenameLibrary:
    """One native-library shape exposing only the required rename primitive."""

    renameatx_np: _RenameOperation


def _state(tmp_path: Path) -> _PublicationState:
    """Build direct staged-publication state.

    Returns:
        A state bound to an otherwise unused local output name.

    """
    destination = bind_destination(str(tmp_path / "output.eml"))
    return staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct terminal receipt.
        destination, b"candidate", "candidate-digest"
    )


def _receipt(state: _PublicationState) -> PublicationReceipt:
    """Build a complete visible receipt for a direct terminal test.

    Returns:
        One immutable receipt tied to the state's requested destination.

    """
    return PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(1, 2, "regular", 3),
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="pending",
    )


def test_darwin_rename_binds_errno_aware_native_call_and_exact_arguments(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The Darwin primitive must capture errno and use both directory-relative names."""
    operation = _RenameOperation(0)
    library = _RenameLibrary(operation)
    calls: list[tuple[object, object]] = []

    def load(name: object, *, use_errno: object) -> _RenameLibrary:
        calls.append((name, use_errno))
        return library

    monkeypatch.setattr(atomic_publish.__dict__["ctypes"], "CDLL", load)
    atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - Darwin no-replace ABI receipt.
        41, b"stage", b"final"
    )

    assert calls == [(None, True)]
    assert operation.argtypes == [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    assert operation.restype is ctypes.c_int
    assert operation.calls == [(41, b"stage", 41, b"final", atomic_publish.RENAME_EXCL)]


def test_darwin_and_link_collisions_keep_the_exact_conflict_receipt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Both no-replace mechanisms report an occupied destination identically."""
    operation = _RenameOperation(-1)
    monkeypatch.setattr(
        atomic_publish.__dict__["ctypes"],
        "CDLL",
        lambda *_args, **_kwargs: _RenameLibrary(operation),
    )
    monkeypatch.setattr(
        atomic_publish.__dict__["ctypes"], "get_errno", lambda: errno.EEXIST
    )
    with pytest.raises(AppError) as darwin:
        atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - native occupied destination.
            41, b"stage", b"final"
        )
    assert darwin.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination already exists"
    )

    conflict = FileExistsError("occupied")
    monkeypatch.setattr(
        atomic_publish.__dict__["os"],
        "link",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(conflict),
    )
    with pytest.raises(AppError) as portable:
        atomic_publish._link_exclusive(  # ruff: ignore[private-member-access] - portable occupied destination.
            41, b"stage", b"final"
        )
    assert portable.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination already exists"
    )
    assert portable.value.__cause__ is conflict


def test_atomic_publish_failure_and_directory_sync_keep_exact_os_receipts(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unexpected native failures retain their concrete OS context in public errors."""
    operation = _RenameOperation(-1)
    failure = OSError(errno.ENOSPC, "no space")
    monkeypatch.setattr(
        atomic_publish.__dict__["ctypes"],
        "CDLL",
        lambda *_args, **_kwargs: _RenameLibrary(operation),
    )
    monkeypatch.setattr(
        atomic_publish.__dict__["ctypes"], "get_errno", lambda: errno.ENOSPC
    )
    monkeypatch.setattr(
        atomic_publish.__dict__["os"], "strerror", lambda _errno: "no space"
    )
    with pytest.raises(AppError) as rename:
        atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - concrete Darwin failure.
            41, b"stage", b"final"
        )
    assert rename.value == AppError(
        ExitCode.WRITE_ERROR, "could not publish candidate: no space"
    )

    monkeypatch.setattr(
        atomic_publish.__dict__["os"],
        "fsync",
        lambda _fd: (_ for _ in ()).throw(failure),
    )
    with pytest.raises(AppError) as sync:
        atomic_publish.sync_directory(41)
    assert sync.value == AppError(
        ExitCode.WRITE_ERROR,
        "could not sync destination directory: [Errno 28] no space",
    )
    assert sync.value.__cause__ is failure


def test_published_cleanup_failure_retains_both_receipt_and_cause(
    tmp_path: Path,
) -> None:
    """A cleanup exception carries the finalized receipt and original cause."""
    state = _state(tmp_path)
    state.kernel_published = True
    state.receipt = _receipt(state)
    cleanup = RuntimeError("cleanup failed")

    with pytest.raises(PublishedWithError) as raised:
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - post-edge cleanup receipt.
            state, None, ("failed", cleanup)
        )
    assert raised.value.cause is cleanup
    assert raised.value.cleanup_cause is None
    assert raised.value.receipt.identity == state.receipt.identity
    assert raised.value.receipt.temp_cleanup == "failed"


def test_private_stage_removal_passes_its_exact_owned_descriptor(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Cleanup removes only the owned stage name through its matching descriptor."""
    state = _state(tmp_path)
    state.parent = BoundDirectory(41, windows=False)
    state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - direct owned entry.
    calls: list[tuple[BoundDirectory, int, bytes]] = []
    monkeypatch.setattr(
        staged_output,
        "discard_private_stage",
        lambda parent, descriptor, name: calls.append((parent, descriptor, name)),
    )

    assert staged_output._remove_stage_entry(state) is None  # ruff: ignore[private-member-access] - exact private unlink receipt.
    assert calls == [(state.parent, 51, b"stage")]
    assert state.stage.name is None


def test_unpublished_windows_stage_is_still_removed_by_exact_name(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Windows avoids post-publish deletion only after the kernel edge occurred."""
    state = _state(tmp_path)
    state.parent = BoundDirectory(41, windows=True)
    state.stage = staged_output._Stage(51, "stage")  # ruff: ignore[private-member-access] - direct owned Windows entry.
    calls: list[tuple[BoundDirectory, int, str]] = []
    monkeypatch.setattr(
        staged_output,
        "discard_private_stage",
        lambda parent, descriptor, name: calls.append((parent, descriptor, name)),
    )

    assert staged_output._remove_stage_entry(state) is None  # ruff: ignore[private-member-access] - unpublished Windows stage must be discarded.
    assert calls == [(state.parent, 51, "stage")]
    assert state.stage.name is None


def test_report_requires_terminal_rows_and_accumulates_repeated_statuses() -> None:
    """Reports reject pending ledgers and count every terminal row rather than one."""
    pending = BatchLedger.from_requests([PathValue("one", "one", "b25l")])
    with pytest.raises(
        RuntimeError, match="report requested before ledger terminalization"
    ) as raised:
        reporting_v3.report(pending, "apply", 9)
    assert str(raised.value) == "report requested before ledger terminalization"

    ledger = BatchLedger.from_requests([
        PathValue("one", "one", "b25l"),
        PathValue("two", "two", "dHdv"),
    ])
    for item in ledger.items:
        item.finish(ItemStatus.CREATED)
    document = reporting_v3.report(ledger, "apply", 0)
    assert document["summary"] == {
        "created": 2,
        "existing_verified": 0,
        "would_create": 0,
        "failed": 0,
        "cancelled": 0,
        "not_run": 0,
        "published_with_error": 0,
        "total": 2,
    }


def test_human_reporting_rejects_nonschema_items_with_exact_error() -> None:
    """The public human renderer names its malformed report-field violation exactly."""
    with pytest.raises(TypeError, match="report items are not a list") as raised:
        reporting_v3.write_human({"items": "not-a-list"})
    assert str(raised.value) == "report items are not a list"


def test_paths0_never_emits_an_unaccepted_final_address(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """A final address alone cannot make a failed ledger item externally accepted."""
    source = PathValue("source", "source", "c291cmNl")
    item = LedgerItem(0, source)
    item.publication = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(1, 2, "regular", 3),
        digest="digest",
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=PathValue("unexpected", "unexpected", "dW5leHBlY3RlZA=="),
        temp_cleanup="succeeded",
    )
    item.finish(ItemStatus.FAILED, AppError(ExitCode.WRITE_ERROR, "failed"))
    reporting_v3.write_paths0(BatchLedger([item]))
    output, errors = capfd.readouterr()
    assert not output
    assert errors == "source: WRITE_ERROR: failed\n"

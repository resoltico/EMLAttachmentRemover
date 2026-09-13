"""Independent exact receipts for binding and POSIX source operations."""

from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass, field
from functools import partial
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import native_binding, native_posix
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExistingEntry,
    ExitCode,
    FileIdentity,
    PathValue,
    SourceSnapshot,
)

if TYPE_CHECKING:
    from collections.abc import Iterator


def _identity(*, inode: int = 12, changed: int = 13) -> FileIdentity:
    return FileIdentity(11, inode, "regular", changed)


def _destination() -> BoundDestination:
    value = PathValue("out.eml", "out.eml", "b3V0LmVtbA==")
    return BoundDestination(value, value, b"out.eml", _identity())


@pytest.mark.parametrize(
    ("platform", "backend"),
    [
        ("posix", native_binding.__dict__["_posix"]),
        ("nt", native_binding.__dict__["_windows"]),
    ],
)
def test_binding_dispatch_preserves_request_and_expansion_receipts(
    platform: str,
    backend: object,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Only source binding receives the exact intended request representation."""
    calls: list[tuple[str, tuple[object, ...]]] = []

    def bind(request: str, expanded: str) -> str:
        calls.append(("bind", (request, expanded)))
        return "bound"

    def inspect(expanded: str) -> str:
        calls.append(("inspect", (expanded,)))
        return "identity"

    def read(request: str, expanded: str) -> str:
        calls.append(("read", (request, expanded)))
        return "snapshot"

    monkeypatch.setattr(native_binding.__dict__["os"], "name", platform)
    monkeypatch.setattr(
        native_binding, "validate_argument", lambda request: f"expanded:{request}"
    )
    monkeypatch.setattr(backend, "bind_destination", bind)
    monkeypatch.setattr(backend, "inspect_source_identity", inspect)
    monkeypatch.setattr(backend, "read_source", read)

    assert cast("object", native_binding.bind_destination("requested")) == "bound"
    assert (
        cast("object", native_binding.inspect_source_identity("requested"))
        == "identity"
    )
    assert cast("object", native_binding.read_source("requested")) == "snapshot"
    assert calls == [
        ("bind", ("requested", "expanded:requested")),
        ("inspect", ("expanded:requested",)),
        ("read", ("requested", "expanded:requested")),
    ]


@dataclass
class NativeReceiptRecorder:
    """Record the precise public binding call sent to a native backend."""

    calls: list[tuple[str, tuple[object, ...]]] = field(default_factory=list)

    def open_bound_destination(self, destination: BoundDestination) -> str:
        self.calls.append(("open", (destination,)))
        return "opened"

    def close_bound_directory(self, value: BoundDirectory) -> None:
        self.calls.append(("close", (value,)))

    def create_private_stage(self, value: BoundDirectory, name: bytes | str) -> int:
        self.calls.append(("create", (value, name)))
        return 48

    def publish_stage_no_replace(
        self,
        value: BoundDirectory,
        descriptor: int,
        stage: bytes | str,
        final: bytes | str,
    ) -> bool:
        self.calls.append(("publish", (value, descriptor, stage, final)))
        return True

    def child_lstat(self, value: BoundDirectory, name: bytes | str) -> FileIdentity:
        self.calls.append(("child", (value, name)))
        return _identity()

    def open_child_nofollow(self, value: BoundDirectory, name: bytes | str) -> int:
        self.calls.append(("open-child", (value, name)))
        return 49

    def discard_private_stage(
        self, value: BoundDirectory, descriptor: int, name: bytes | str
    ) -> None:
        self.calls.append(("discard", (value, descriptor, name)))

    def sync_bound_directory(self, value: BoundDirectory) -> str:
        self.calls.append(("sync", (value,)))
        return "succeeded"


@dataclass(frozen=True)
class NativeDispatchCase:
    """One platform's exact directory and native-name values."""

    platform: str
    backend: object
    directory: BoundDirectory
    stage_name: bytes | str
    destination_name: bytes | str


@pytest.mark.parametrize(
    "case",
    [
        NativeDispatchCase(
            "posix",
            native_binding.__dict__["_posix"],
            BoundDirectory(47, windows=False),
            b"stage.tmp",
            b"final.eml",
        ),
        NativeDispatchCase(
            "nt",
            native_binding.__dict__["_windows"],
            BoundDirectory(47, windows=True),
            "stage.tmp",
            "final.eml",
        ),
    ],
)
def test_binding_dispatch_preserves_directory_descriptor_and_native_names(
    case: NativeDispatchCase,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Publication wrappers retain one bound handle and native name type end to end."""
    recorder = NativeReceiptRecorder()

    monkeypatch.setattr(native_binding.__dict__["os"], "name", case.platform)
    monkeypatch.setattr(
        case.backend, "open_bound_destination", recorder.open_bound_destination
    )
    monkeypatch.setattr(
        case.backend, "close_bound_directory", recorder.close_bound_directory
    )
    monkeypatch.setattr(
        case.backend, "create_private_stage", recorder.create_private_stage
    )
    monkeypatch.setattr(
        case.backend, "publish_stage_no_replace", recorder.publish_stage_no_replace
    )
    monkeypatch.setattr(case.backend, "child_lstat", recorder.child_lstat)
    monkeypatch.setattr(
        case.backend, "open_child_nofollow", recorder.open_child_nofollow
    )
    monkeypatch.setattr(
        case.backend, "discard_private_stage", recorder.discard_private_stage
    )
    monkeypatch.setattr(
        case.backend, "sync_bound_directory", recorder.sync_bound_directory
    )

    destination = _destination()
    assert (
        cast("object", native_binding.open_bound_destination(destination)) == "opened"
    )
    native_binding.close_bound_directory(case.directory)
    assert native_binding.create_private_stage(case.directory, case.stage_name) == 48
    assert (
        native_binding.publish_stage_no_replace(
            case.directory, 48, case.stage_name, case.destination_name
        )
        is True
    )
    assert (
        native_binding.child_lstat(case.directory, case.destination_name) == _identity()
    )
    assert (
        native_binding.open_child_nofollow(case.directory, case.destination_name) == 49
    )
    native_binding.discard_private_stage(case.directory, 48, case.stage_name)
    assert native_binding.sync_bound_directory(case.directory) == "succeeded"
    assert recorder.calls == [
        ("open", (destination,)),
        ("close", (case.directory,)),
        ("create", (case.directory, case.stage_name)),
        ("publish", (case.directory, 48, case.stage_name, case.destination_name)),
        ("child", (case.directory, case.destination_name)),
        ("open-child", (case.directory, case.destination_name)),
        ("discard", (case.directory, 48, case.stage_name)),
        ("sync", (case.directory,)),
    ]


def test_binding_existing_read_keeps_every_stability_receipt_independent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An existing file must retain descriptor and name identity through its read."""
    destination = _destination()
    directory = BoundDirectory(97, windows=False)
    original = _identity()
    changed = _identity(inode=99)

    for after, child, expected_calls in (
        (
            original,
            original,
            ["open", "open-child", "identity", "read", "identity", "child"],
        ),
        (changed, original, ["open", "open-child", "identity", "read", "identity"]),
        (
            original,
            changed,
            ["open", "open-child", "identity", "read", "identity", "child"],
        ),
    ):
        calls: list[str] = []
        closed: list[int] = []
        identities = iter((original, after))
        monkeypatch.setattr(
            native_binding,
            "_open_bound_destination",
            partial(_record_open, calls, directory),
        )
        monkeypatch.setattr(
            native_binding,
            "_open_child_nofollow",
            partial(_record_child_open, calls),
        )
        monkeypatch.setattr(
            native_binding,
            "_descriptor_identity",
            partial(_record_identity, calls, identities),
        )
        monkeypatch.setattr(
            native_binding,
            "_read_all",
            partial(_record_read, calls),
        )
        monkeypatch.setattr(
            native_binding,
            "_child_lstat",
            partial(_record_child, calls, child),
        )
        monkeypatch.setattr(native_binding.__dict__["os"], "close", closed.append)
        monkeypatch.setattr(
            native_binding,
            "_close_bound_directory",
            partial(_record_directory_close, closed),
        )

        if after == original and child == original:
            assert native_binding.read_existing(destination) == ExistingEntry(
                original, b"existing"
            )
        else:
            with pytest.raises(AppError) as captured:
                native_binding.read_existing(destination)
            assert captured.value == AppError(
                ExitCode.OUTPUT_CONFLICT,
                "existing output changed while verified",
            )
        assert calls == expected_calls
        assert closed == [98, 97]


def test_binding_read_all_requests_exact_remaining_budget_and_conflict(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Existing-output reads use a bounded one-byte lookahead at every boundary."""
    requests: list[tuple[int, int]] = []
    chunks = iter((b"ab", b"c", b""))

    def read(descriptor: int, count: int) -> bytes:
        requests.append((descriptor, count))
        return next(chunks)

    monkeypatch.setattr(native_binding, "MAX_RAW_BYTES", 3)
    monkeypatch.setattr(native_binding.__dict__["os"], "read", read)
    assert native_binding._read_all(53) == b"abc"  # ruff: ignore[private-member-access] - direct bounded-existing-output receipt.
    assert requests == [(53, 4), (53, 2), (53, 1)]

    oversized = iter((b"abc", b"d"))
    monkeypatch.setattr(
        native_binding.__dict__["os"], "read", lambda *_args: next(oversized)
    )
    with pytest.raises(AppError) as captured:
        native_binding._read_all(53)  # ruff: ignore[private-member-access] - direct over-limit existing-output receipt.
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "existing output exceeds source limit"
    )


def test_posix_source_read_preserves_exact_child_open_and_snapshot_handoff(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Source reading passes a captured parent, byte basename, and opened descriptor."""
    opens: list[tuple[object, object, object]] = []
    closed: list[int] = []
    snapshots: list[tuple[object, ...]] = []
    marker = cast("SourceSnapshot", object())

    def open_source(name: object, flags: object, *, dir_fd: object) -> int:
        opens.append((name, flags, dir_fd))
        return 72

    def snapshot(*values: object) -> SourceSnapshot:
        snapshots.append(values)
        return marker

    monkeypatch.setattr(native_posix, "_parent", lambda _path: (71, "parent", b"x"))
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_source)
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)
    monkeypatch.setattr(native_posix, "_snapshot", snapshot)

    assert native_posix.read_source("request", "expanded") is marker
    expected_flags = (
        os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_CLOEXEC", 0)
    )
    assert opens == [(b"x", expected_flags, 71)]
    assert snapshots == [("request", "expanded", "parent", b"x", 72)]
    assert closed == [72, 71]


def test_posix_snapshot_preserves_all_address_metadata_and_payload_evidence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The accepted snapshot is a complete record tied to one stable descriptor."""
    metadata = SimpleNamespace(
        st_dev=21,
        st_ino=22,
        st_mode=0o100640,
        st_ctime_ns=23,
        st_size=3,
        st_mtime_ns=24,
    )
    values = iter((metadata, metadata))
    path_calls: list[str] = []
    descriptor_calls: list[int] = []
    final = PathValue("final", "final", "ZmluYWw=")

    def value(text: str) -> PathValue:
        path_calls.append(text)
        return PathValue(text, f"display:{text}", f"native:{text}")

    def fstat(descriptor: int) -> SimpleNamespace:
        descriptor_calls.append(descriptor)
        return next(values)

    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", fstat)
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_ISREG", lambda _mode: True)
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_IMODE", lambda _mode: 0o640)
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "filemode", lambda _mode: "-rw-r-----"
    )
    monkeypatch.setattr(native_posix, "_read_all", lambda _descriptor: b"raw")
    monkeypatch.setattr(native_posix, "_final_address", lambda _path: final)
    monkeypatch.setattr(native_posix, "path_value", value)

    assert native_posix._snapshot(  # ruff: ignore[private-member-access] - complete accepted POSIX snapshot receipt.
        "request", "expanded", "parent", b"leaf", 73
    ) == SourceSnapshot(
        PathValue("request", "display:request", "native:request"),
        PathValue("expanded", "display:expanded", "native:expanded"),
        PathValue("parent", "display:parent", "native:parent"),
        b"leaf",
        final,
        FileIdentity(21, 22, "-rw-r-----", 23),
        0o640,
        b"raw",
        hashlib.sha256(b"raw").hexdigest(),
        3,
    )
    assert descriptor_calls == [73, 73]
    assert path_calls == ["request", "expanded", "parent"]


def _record_open(
    calls: list[str], directory: BoundDirectory, value: BoundDestination
) -> BoundDirectory:
    assert value == _destination()
    calls.append("open")
    return directory


def _record_child_open(calls: list[str], value: BoundDirectory, name: bytes) -> int:
    assert value == BoundDirectory(97, windows=False)
    assert name == b"out.eml"
    calls.append("open-child")
    return 98


def _record_identity(
    calls: list[str], values: Iterator[FileIdentity], descriptor: int
) -> FileIdentity:
    assert descriptor == 98
    calls.append("identity")
    return next(values)


def _record_read(calls: list[str], descriptor: int) -> bytes:
    assert descriptor == 98
    calls.append("read")
    return b"existing"


def _record_child(
    calls: list[str], identity: FileIdentity, value: BoundDirectory, name: bytes
) -> FileIdentity:
    assert value == BoundDirectory(97, windows=False)
    assert name == b"out.eml"
    calls.append("child")
    return identity


def _record_directory_close(closed: list[int], value: BoundDirectory) -> None:
    closed.append(value.descriptor)

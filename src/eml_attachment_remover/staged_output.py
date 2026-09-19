"""Private staging, no-replace publication, and post-edge reconciliation."""

from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass, replace

from . import native_literal_address, staged_receipt, staged_signals
from .domain import (
    AppError,
    BoundDestination,
    ExitCode,
    PublicationReceipt,
)
from .native_paths import (
    BoundDirectoryHandle,
    child_lstat,
    close_bound_directory,
    create_private_stage,
    descriptor_identity,
    discard_private_stage,
    final_address,
    open_bound_destination,
    open_child_nofollow,
    private_stage_name,
    publish_stage_no_replace,
    sync_bound_directory,
)
from .staged_progress import advance_position

_defer_signals = staged_signals.defer_signals

type _LifecycleOutcome = tuple[BaseException | None, tuple[str, BaseException | None]]


def _combined(errors: list[BaseException]) -> BaseException:
    """Preserve all cleanup failures instead of overwriting an earlier cause."""  # ruff: ignore[docstring-missing-returns] - error combiner.
    if len(errors) == 1:
        return errors[0]
    return BaseExceptionGroup("multiple staging cleanup failures", errors)


@dataclass(slots=True)
class PublishedWithError(Exception):
    """A post-edge failure accompanied by the strongest available receipt."""

    receipt: PublicationReceipt
    cause: BaseException
    cleanup_cause: BaseException | None = None


@dataclass(slots=True)
class _Stage:
    """The one private directory entry and descriptor owned by one publication."""

    descriptor: int
    name: bytes | str | None


@dataclass(slots=True)
class _PublicationState:
    """Mutable handles plus immutable candidate facts for one bounded lifecycle."""

    destination: BoundDestination
    candidate: bytes
    digest: str
    parent: BoundDirectoryHandle | None = None
    stage: _Stage | None = None
    kernel_published: bool = False
    receipt: PublicationReceipt | None = None


def _bind_parent(state: _PublicationState) -> None:
    """Open and immediately transfer ownership of the bound destination directory."""
    state.parent = open_bound_destination(state.destination)


def _create_stage(
    state: _PublicationState,
) -> None:
    """Create an exclusive stage and transfer cleanup ownership before returning.

    Raises:
        AppError: If every bounded unpredictable staging-name attempt collides.

    """
    parent = state.parent
    if parent is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "destination directory was not bound")
    for _attempt in range(16):
        name = private_stage_name()
        try:
            descriptor = create_private_stage(parent, name)
        except FileExistsError:
            continue
        try:
            state.stage = _Stage(descriptor, name)
        except BaseException as primary:  # ruff: ignore[blind-except] - preserve cancellation during ownership transfer.
            cleanup_errors = [primary]
            try:
                discard_private_stage(parent, descriptor, name)
            except FileNotFoundError:
                pass
            except BaseException as cleanup:  # ruff: ignore[blind-except] - preserve nested cancellation.
                cleanup_errors.append(cleanup)
            close_problem = _close_descriptor(descriptor)
            if close_problem is not None:
                cleanup_errors.append(close_problem)
            _raise_stage_construction_failures(cleanup_errors)
        return
    raise AppError(ExitCode.WRITE_ERROR, "could not allocate a private staging name")


def _raise_stage_construction_failures(errors: list[BaseException]) -> None:
    """Raise a sole construction failure directly or preserve every extra failure.

    Raises:
        _combined: If construction and cleanup fail independently.

    """
    primary = errors[0]
    if len(errors) == 1:
        raise primary
    raise _combined(errors) from primary


def _read_all(descriptor: int) -> bytes:
    """Read one descriptor to EOF through bounded chunks."""  # ruff: ignore[docstring-missing-returns] - direct descriptor read.
    chunks: list[bytes] = []
    while chunk := os.read(descriptor, 1024 * 1024):
        chunks.append(chunk)
    return b"".join(chunks)


def _close_descriptor(descriptor: int) -> BaseException | None:
    """Close one descriptor while leaving independent cleanup work reachable."""  # ruff: ignore[docstring-missing-returns] - cleanup result is obvious.
    if descriptor < 0:
        return None
    try:
        os.close(descriptor)
    except BaseException as exc:  # ruff: ignore[blind-except] - cleanup preserves cancellation too.
        return exc
    return None


def _close_directory(directory: BoundDirectoryHandle) -> BaseException | None:
    """Close one bound parent without masking the independent stage close.

    Returns:
        The caught native-handle close failure, or ``None`` when closure succeeded.

    """
    try:
        close_bound_directory(directory)
    except BaseException as exc:  # ruff: ignore[blind-except] - cleanup preserves cancellation too.
        return exc
    return None


def _verify_staged(state: _PublicationState) -> None:
    """Verify the staged candidate's sync, mode, and reread receipt.

    Raises:
        AppError: If staging evidence is absent or differs from the candidate.

    """
    stage = state.stage
    parent = state.parent
    if stage is None or parent is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "staging file was not created")
    position = 0
    while position < len(state.candidate):
        written = os.write(stage.descriptor, state.candidate[position:])
        position = advance_position(position, written, len(state.candidate))
    os.fsync(stage.descriptor)
    if not parent.windows:
        os.fchmod(stage.descriptor, 0o600)
    os.lseek(stage.descriptor, 0, os.SEEK_SET)
    if _read_all(stage.descriptor) != state.candidate:
        raise AppError(
            ExitCode.VERIFICATION_ERROR, "staged candidate bytes do not match"
        )


def _read_final_receipt(state: _PublicationState) -> PublicationReceipt:
    """Delegate final-address proof while retaining this lifecycle's live operations.

    Returns:
        The complete visible final receipt.

    """
    return staged_receipt.read_final_receipt(
        state,
        staged_receipt.ReceiptOperations(
            descriptor_identity,
            child_lstat,
            open_child_nofollow,
            final_address,
            native_literal_address.open_final_address,
            _read_all,
            _close_descriptor,
        ),
    )


def _reconcile(state: _PublicationState, directory_sync: str) -> PublicationReceipt:
    """Record the strongest truthful receipt while both main fds are live.

    Returns:
        A verified receipt when possible, otherwise an explicitly unproven receipt.

    """
    try:
        receipt = _read_final_receipt(state)
    except BaseException:  # ruff: ignore[blind-except] - failure itself is receipt evidence.
        stage = state.stage
        identity = None if stage is None else descriptor_identity(stage.descriptor)
        return PublicationReceipt(
            visibility="not_proven",
            identity=identity,
            digest=state.digest,
            file_sync="succeeded",
            directory_sync=directory_sync,
            address_verified=False,
            final_address=None,
            temp_cleanup="pending",
        )
    return replace(receipt, directory_sync=directory_sync)


def _remove_stage_entry(state: _PublicationState) -> BaseException | None:
    """Remove only the exact entry owned by this lifecycle, never a prefix glob.

    Returns:
        A caught removal failure, or ``None`` when the exact private entry is absent.

    """
    stage = state.stage
    parent = state.parent
    if stage is None or stage.name is None or parent is None:
        return None
    if state.kernel_published and parent.windows:
        stage.name = None
        return None
    try:
        discard_private_stage(parent, stage.descriptor, stage.name)
    except FileNotFoundError:
        stage.name = None
    except BaseException as exc:  # ruff: ignore[blind-except] - cleanup must retain cancellation.
        return exc
    else:
        stage.name = None
    return None


def _cleanup(state: _PublicationState) -> tuple[str, BaseException | None]:
    """Attempt entry and both descriptor cleanups independently and exactly once.

    Returns:
        The staging-entry outcome and every independently observed cleanup cause.

    """
    failures: list[BaseException] = []
    entry_problem = _remove_stage_entry(state)
    if entry_problem is not None:
        failures.append(entry_problem)
    stage = state.stage
    if stage is not None:
        stage_problem = _close_descriptor(stage.descriptor)
        stage.descriptor = -1
        if stage_problem is not None:
            failures.append(stage_problem)
    parent = state.parent
    parent_problem = None if parent is None else _close_directory(parent)
    state.parent = None
    if parent_problem is not None:
        failures.append(parent_problem)
    return (
        "failed" if entry_problem is not None else "succeeded",
        None if not failures else _combined(failures),
    )


def _publish_edge(state: _PublicationState) -> None:
    """Cross the visibility edge then obtain a durable, live-handle address receipt.

    Raises:
        AppError: If staging ownership, publication, sync, or re-addressing fails.

    """
    stage = state.stage
    parent = state.parent
    if stage is None or parent is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "staging file was not created")
    if stage.name is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "staging name is unavailable")
    stage_link_remains = publish_stage_no_replace(
        parent, stage.descriptor, stage.name, state.destination.basename
    )
    state.kernel_published = True
    _read_final_receipt(state)
    directory_sync = sync_bound_directory(parent)
    if stage_link_remains:
        cleanup_problem = _remove_stage_entry(state)
        if cleanup_problem is not None:
            raise cleanup_problem
        directory_sync = sync_bound_directory(parent)
    state.receipt = _reconcile(state, directory_sync)
    if not state.receipt.address_verified:
        raise AppError(ExitCode.WRITE_ERROR, "could not re-address published candidate")


def _finish_or_raise(
    state: _PublicationState,
    primary: BaseException | None,
    cleanup: tuple[str, BaseException | None],
) -> PublicationReceipt:
    """Return only a fully accepted result or preserve every post-edge failure.

    Returns:
        The accepted, durable, final-address publication receipt.

    Raises:
        AppError: If pre-edge work or cleanup could not be accepted.
        PublishedWithError: If a visible candidate lacks a complete final receipt.
        _combined: If both a pre-edge cause and cleanup cause must be preserved.

    """
    receipt = state.receipt
    if state.kernel_published:
        if receipt is None:
            receipt = _reconcile(state, "failed")
        receipt = replace(receipt, temp_cleanup=cleanup[0])
        if primary is not None:
            raise PublishedWithError(receipt, primary, cleanup[1])
        if cleanup[1] is not None:
            raise PublishedWithError(receipt, cleanup[1])
        return receipt
    if primary is not None and cleanup[1] is not None:
        raise _combined([primary, cleanup[1]])
    if primary is not None:
        raise primary
    if cleanup[1] is not None:
        raise cleanup[1]
    raise AppError(
        ExitCode.INTERNAL_ERROR, "publication ended before its visibility edge"
    )


def _run_lifecycle(state: _PublicationState) -> _LifecycleOutcome:
    """Run the publication interior and retain its terminal cleanup outcome.

    Returns:
        The pre-edge failure, if any, and exact cleanup outcome.

    """
    primary: BaseException | None = None
    try:
        _bind_parent(state)
        _create_stage(state)
        _verify_staged(state)
        _publish_edge(state)
    except BaseException as exc:  # ruff: ignore[blind-except] - preserve cancellation and exit.
        primary = exc
        if state.kernel_published and state.receipt is None:
            state.receipt = _reconcile(state, "failed")
    return primary, _cleanup(state)


def publish(destination: BoundDestination, candidate: bytes) -> PublicationReceipt:
    """Stage, verify, publish without replacement, and reconcile one candidate.

    Returns:
        A final-address receipt only after all required cleanup has been accepted.

    """
    state = _PublicationState(
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )
    completed: list[_LifecycleOutcome] = []
    try:
        with _defer_signals():
            completed.append(_run_lifecycle(state))
    except BaseException as deferred:
        if not completed:
            raise
        primary, cleanup = completed[0]
        primary = deferred if primary is None else _combined([primary, deferred])
        return _finish_or_raise(state, primary, cleanup)
    primary, cleanup = completed[0]
    return _finish_or_raise(state, primary, cleanup)

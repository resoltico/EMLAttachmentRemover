"""Literal-address final-receipt proof for one already-published candidate."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Protocol

from .domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
    PublicationReceipt,
)

if TYPE_CHECKING:
    from collections.abc import Callable


class _Stage(Protocol):
    """Describe the active private descriptor required for final receipt proof."""

    descriptor: int


class PublicationState(Protocol):
    """Describe the active publication facts consumed by receipt verification."""

    destination: BoundDestination
    digest: str
    parent: BoundDirectory | None

    @property
    def stage(self) -> _Stage | None:
        """The active private stage receipt."""


@dataclass(frozen=True, slots=True)
class ReceiptOperations:
    """Inject native evidence and cleanup operations for one final receipt."""

    descriptor_identity: Callable[[int], FileIdentity]
    child_lstat: Callable[[BoundDirectory, bytes | str], FileIdentity | None]
    open_child_nofollow: Callable[[BoundDirectory, bytes | str], int]
    final_address: Callable[[int], PathValue | None]
    open_final_address: Callable[[PathValue], int]
    read_all: Callable[[int], bytes]
    close_descriptor: Callable[[int], BaseException | None]


@dataclass(frozen=True, slots=True)
class _ReceiptContext:
    """Bundle final-entry facts shared by literal and descriptor proof phases."""

    parent: BoundDirectory
    destination: BoundDestination
    expected: FileIdentity
    digest: str
    operations: ReceiptOperations


def _combined(errors: list[BaseException]) -> BaseException:
    """Return a sole cleanup failure directly or preserve each independent failure.

    Returns:
        The sole problem or an exception group preserving each problem.

    """
    return (
        errors[0]
        if len(errors) == 1
        else BaseExceptionGroup("multiple staging cleanup failures", errors)
    )


def _literal_matches(descriptor: int, context: _ReceiptContext) -> bool:
    """Return whether a literal no-follow reopen proves the bound final object.

    Returns:
        ``True`` only when regularity, identity, digest, and entry facts all agree.

    """
    operations = context.operations
    identity = operations.descriptor_identity(descriptor)
    observed = hashlib.sha256(operations.read_all(descriptor)).hexdigest()
    entry = operations.child_lstat(context.parent, context.destination.basename)
    return (
        identity.is_regular()
        and identity == context.expected
        and observed == context.digest
        and entry == context.expected
    )


def _verify_literal_address(address: PathValue, context: _ReceiptContext) -> None:
    """Reopen and prove one literal final path while preserving close failures.

    Raises:
        AppError: If the literal address no longer resolves to the published object.
        _combined: If proof and descriptor cleanup fail independently.

    """
    descriptor = -1
    try:
        descriptor = context.operations.open_final_address(address)
        if not _literal_matches(descriptor, context):
            # ruff: ignore[raise-within-try] - the literal descriptor needs common cleanup.
            raise AppError(
                ExitCode.VERIFICATION_ERROR,
                "final address did not resolve to published destination",
            )
    except BaseException as problem:
        close_problem = context.operations.close_descriptor(descriptor)
        descriptor = -1
        if close_problem is not None:
            raise _combined([problem, close_problem])  # ruff: ignore[raise-without-from-inside-except] - preserve both native failures.
        raise
    close_problem = context.operations.close_descriptor(descriptor)
    if close_problem is not None:
        raise close_problem


def read_final_receipt(  # ruff: ignore[complex-structure,too-many-branches]
    state: PublicationState, operations: ReceiptOperations
) -> PublicationReceipt:
    """Prove one published candidate's bound entry and literal final address.

    Returns:
        The complete visible publication receipt.

    Raises:
        AppError: If any native evidence cannot prove the final candidate.
        _combined: If evidence and descriptor cleanup fail independently.

    """
    stage = state.stage
    parent = state.parent
    if stage is None or parent is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "staging file was not created")
    descriptor = -1
    try:  # ruff: ignore[too-many-statements-in-try-clause] - ordered evidence shares descriptor cleanup.
        expected = operations.descriptor_identity(stage.descriptor)
        context = _ReceiptContext(
            parent, state.destination, expected, state.digest, operations
        )
        entry = operations.child_lstat(parent, state.destination.basename)
        if entry is None or not entry.is_regular() or entry != expected:
            # ruff: ignore[raise-within-try] - the final descriptor needs common cleanup.
            raise AppError(
                ExitCode.WRITE_ERROR, "published destination identity mismatch"
            )
        descriptor = operations.open_child_nofollow(parent, state.destination.basename)
        if operations.descriptor_identity(descriptor) != expected:
            # ruff: ignore[raise-within-try] - the final descriptor needs common cleanup.
            raise AppError(ExitCode.WRITE_ERROR, "final destination handle changed")
        observed = hashlib.sha256(operations.read_all(descriptor)).hexdigest()
        if (
            operations.descriptor_identity(descriptor) != expected
            or observed != state.digest
        ):
            # ruff: ignore[raise-within-try] - the final descriptor needs common cleanup.
            raise AppError(ExitCode.VERIFICATION_ERROR, "final destination changed")
        if operations.child_lstat(parent, state.destination.basename) != expected:
            # ruff: ignore[raise-within-try] - the final descriptor needs common cleanup.
            raise AppError(ExitCode.WRITE_ERROR, "final destination entry changed")
        address = operations.final_address(descriptor)
        if address is None:
            # ruff: ignore[raise-within-try] - the final descriptor needs common cleanup.
            raise AppError(
                ExitCode.WRITE_ERROR, "could not prove published final address"
            )
        _verify_literal_address(address, context)
        return PublicationReceipt(
            visibility="visible",
            identity=expected,
            digest=observed,
            file_sync="succeeded",
            directory_sync="succeeded",
            address_verified=True,
            final_address=address,
            temp_cleanup="pending",
        )
    except BaseException as problem:
        close_problem = operations.close_descriptor(descriptor)
        descriptor = -1
        if close_problem is not None:
            raise _combined([problem, close_problem])  # ruff: ignore[raise-without-from-inside-except] - preserve both native failures.
        raise
    finally:
        if descriptor >= 0:
            close_problem = operations.close_descriptor(descriptor)
            if close_problem is not None:
                raise close_problem

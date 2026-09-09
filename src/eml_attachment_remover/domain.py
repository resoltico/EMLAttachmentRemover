"""Immutable records used by the MIME-pruned pipeline.

The public report is deliberately assembled from these small records.  Keeping the
ledger separate from MIME parsing prevents an exception in one input from erasing the
history of earlier publications.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import IntEnum, StrEnum
from itertools import starmap
from typing import Final

PROGRAM_NAME: Final = "remove-eml-attachments"
SCHEMA_VERSION: Final = 3
SCOPE: Final = "mime-pruned"
DOUBLE_TERMINAL_ERROR: Final = "attempted to terminalize a ledger item twice"
DOUBLE_INTERRUPTION_ERROR: Final = "attempted to record an interruption twice"

type MimePath = tuple[int, ...]


class ExitCode(IntEnum):
    """Stable process status values."""

    SUCCESS = 0
    USAGE = 2
    INPUT_ERROR = 3
    OUTPUT_CONFLICT = 4
    PARSE_ERROR = 5
    TRANSFORMATION_UNAVAILABLE = 6
    WRITE_ERROR = 7
    VERIFICATION_ERROR = 8
    BATCH_FAILURE = 9
    PUBLICATION_INCOMPLETE = 10
    INTERNAL_ERROR = 70
    INTERRUPTED = 130


class ItemStatus(StrEnum):
    """The one terminal state held by every requested input."""

    CREATED = "created"
    EXISTING_VERIFIED = "existing_verified"
    WOULD_CREATE = "would_create"
    FAILED = "failed"
    CANCELLED = "cancelled"
    NOT_RUN = "not_run"
    PUBLISHED_WITH_ERROR = "published_with_error"


class ItemPhase(StrEnum):
    """The last completed processing phase."""

    REQUESTED = "requested"
    INVENTORIED = "inventoried"
    BOUND = "bound"
    PARSED = "parsed"
    CLASSIFIED = "classified"
    CANDIDATE = "candidate"
    STAGED = "staged"
    PUBLISHED = "published"


class RemovalReason(StrEnum):
    """The closed set of permitted deletion reasons."""

    EXPLICIT_ATTACHMENT = "EXPLICIT_ATTACHMENT"
    RELATED_NONROOT_COMPONENT = "RELATED_NONROOT_COMPONENT"


class DecisionAction(StrEnum):
    """A policy action for one MIME tree node."""

    KEEP = "KEEP"
    REMOVE_SUBTREE = "REMOVE_SUBTREE"
    RECURSE = "RECURSE"
    REJECT = "REJECT"


@dataclass(frozen=True, slots=True)
class AppError(Exception):
    """An expected, source-qualified failure."""

    code: ExitCode
    message: str
    mime_path: MimePath | None = None
    phase: str | None = None

    def __str__(self) -> str:
        """Return the safe error message.

        Returns:
            The stable user-safe expected-error message.

        """
        return self.message


@dataclass(frozen=True, slots=True)
class PathValue:
    """A native argument's public, non-authoritative representation."""

    text: str | None
    display: str
    native_base64: str | None
    native_utf16le_base64: str | None = None


@dataclass(frozen=True, slots=True)
class FileIdentity:
    """Stable identity values obtained from an open POSIX descriptor."""

    device: int
    inode: int
    file_type: str
    ctime_ns: int

    def as_json(self) -> dict[str, str]:
        """Serialize integers as decimal strings for JavaScript-safe JSON.

        Returns:
            Identity fields whose integer values are safe for JSON consumers.

        """
        return {
            "device": str(self.device),
            "inode": str(self.inode),
            "type": self.file_type,
            "ctime_ns": str(self.ctime_ns),
        }


@dataclass(frozen=True, slots=True)
class SourceSnapshot:
    """The exact regular file bytes and descriptor receipts used for planning."""

    request: PathValue
    expanded: PathValue
    parent: PathValue
    basename: bytes | str
    final_address: PathValue | None
    identity: FileIdentity
    mode: int
    raw: bytes
    digest: str
    size: int


@dataclass(frozen=True, slots=True)
class BoundDestination:
    """A destination intent bound to the requested parent directory."""

    request: PathValue
    parent: PathValue
    basename: bytes | str
    directory_identity: FileIdentity


@dataclass(slots=True)
class BoundDirectory:
    """One active platform directory descriptor and its native-handle kind."""

    descriptor: int
    windows: bool


@dataclass(frozen=True, slots=True)
class ExistingEntry:
    """An exact existing candidate body read through a no-follow descriptor."""

    identity: FileIdentity
    raw: bytes = field(repr=False)


@dataclass(frozen=True, slots=True)
class RetainedFingerprint:
    """Encoded and decoded payload evidence for one retained leaf."""

    source_path: MimePath
    content_type: str
    cte: str
    content_type_parameters: tuple[tuple[bytes, bytes], ...]
    encoded_sha256: str
    decoded_sha256: str


@dataclass(frozen=True, slots=True)
class Removal:
    """A source MIME node removed by the closed policy."""

    path: MimePath
    content_type: str
    reason: RemovalReason


@dataclass(frozen=True, slots=True)
class TransformationPlan:
    """All immutable facts needed to construct one candidate byte sequence."""

    removals: tuple[Removal, ...]
    retained: tuple[RetainedFingerprint, ...]
    stripped_headers: tuple[str, ...]
    candidate_sha256: str
    candidate_size: int
    candidate: bytes = field(repr=False)


@dataclass(frozen=True, slots=True)
class VerificationReceipt:
    """Mechanical verification facts, not a claim about visual equivalence."""

    output_parses: bool
    retained_payloads_match: bool
    structure_matches: bool
    policy_is_idempotent: bool
    digest_matches: bool


@dataclass(frozen=True, slots=True)
class InterruptionRecord:
    """The catchable cancellation that ended one batch execution."""

    signal: str
    reason: str
    phase: str


@dataclass(frozen=True, slots=True)
class PublicationReceipt:
    """The observable outcome of staging and no-replace publication."""

    visibility: str
    identity: FileIdentity | None
    digest: str | None
    file_sync: str
    directory_sync: str
    address_verified: bool
    final_address: PathValue | None
    temp_cleanup: str


@dataclass(slots=True)
class LedgerItem:
    """The single mutable truth for an argv input."""

    index: int
    source_request: PathValue
    destination_request: PathValue | None = None
    phase: ItemPhase = ItemPhase.REQUESTED
    status: ItemStatus | None = None
    terminalized: bool = False
    source: SourceSnapshot | None = None
    destination: BoundDestination | None = None
    transformation: TransformationPlan | None = None
    verification: VerificationReceipt | None = None
    publication: PublicationReceipt | None = None
    warnings: list[dict[str, object]] = field(default_factory=list)
    error: AppError | None = None

    def finish(self, status: ItemStatus, error: AppError | None = None) -> None:
        """Set the sole terminal state exactly once.

        Raises:
            RuntimeError: If an already terminal item would be overwritten.

        """
        if self.terminalized or self.status is not None:
            raise RuntimeError(DOUBLE_TERMINAL_ERROR)
        self.status = status
        self.error = error
        self.terminalized = True


@dataclass(slots=True)
class BatchLedger:
    """Preallocated input-order records for one invocation."""

    items: list[LedgerItem]
    interruption: InterruptionRecord | None = None
    batch_error: AppError | None = None

    @classmethod
    def from_requests(cls, requests: list[PathValue]) -> BatchLedger:
        """Allocate every record before filesystem work begins.

        Returns:
            The sole mutable batch ledger with one requested record per argv input.

        """
        return cls(list(starmap(LedgerItem, enumerate(requests))))

    def finalize_not_run(self, reason: str) -> None:
        """Terminalize all untouched inputs with a safe reason."""
        for item in self.items:
            if item.status is None:
                item.finish(
                    ItemStatus.NOT_RUN,
                    AppError(ExitCode.BATCH_FAILURE, reason, phase="batch"),
                )

    def record_interruption(self, signal: str, phase: str) -> None:
        """Record one cancellation and terminalize every not-yet-run input.

        Raises:
            RuntimeError: If a caller attempts to overwrite an interruption receipt.

        """
        if self.interruption is not None:
            raise RuntimeError(DOUBLE_INTERRUPTION_ERROR)
        reason = f"interrupted by {signal}"
        self.interruption = InterruptionRecord(signal, reason, phase)
        for item in self.items:
            if item.status is None:
                item.finish(
                    ItemStatus.NOT_RUN,
                    AppError(ExitCode.INTERRUPTED, reason, phase=phase),
                )

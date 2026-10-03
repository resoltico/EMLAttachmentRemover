"""Input-order inventory, shared candidate construction, and batch ledger control."""

from __future__ import annotations

from base64 import b64encode
from contextlib import ExitStack
from dataclasses import dataclass
from typing import Final

from . import batch_execution, batch_terminal, report_stream
from .cancellation import CancellationSignal, checkpoint, coherent_operation
from .destination_names import fitted_default_destination
from .domain import (
    AppError,
    BatchLedger,
    BoundDestination,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    PublicationReceipt,
    TransformationPlan,
)
from .mime_execution import build_candidate
from .mime_policy import classify
from .mime_raw import parse_raw_mime
from .mime_verification import verify_candidate
from .native_paths import (
    bind_destination,
    existing_identity,
    inspect_source_identity,
    path_value,
    read_existing,
    read_source,
)
from .publication_error import publication_error as _publication_error
from .report_budget import admit, plan
from .staged_output import PublishedWithError, publish

MAX_BATCH_ITEMS: Final = 4_096
MAX_CUMULATIVE_NATIVE_ARGUMENT_BYTES: Final = 4 * 1024 * 1024
MISSING_SOURCE_ADDRESS: Final = "source has no native address"
MISSING_PUBLICATION_INPUTS: Final = "candidate publication lacks destination or plan"


@dataclass(frozen=True, slots=True)
class BatchOptions:
    """CLI-selected behaviors which apply uniformly to each request."""

    dry_run: bool
    existing: str
    fail_fast: bool
    output: str | None
    output_dir: str | None


@dataclass(slots=True)
class _Inventory:
    """Bounded, file-metadata-only facts retained between batch phases."""

    identities: dict[int, FileIdentity]
    source_groups: dict[FileIdentity, list[LedgerItem]]
    destination_groups: dict[tuple[FileIdentity, bytes | str], list[LedgerItem]]

    @classmethod
    def empty(cls) -> _Inventory:
        return cls({}, {}, {})

    def add(self, item: LedgerItem, source: str, options: BatchOptions) -> None:
        """Bind one request and retain its small planning metadata."""
        destination = _destination_for(source, options)
        item.destination_request = path_value(destination)
        identity = inspect_source_identity(source)
        self.identities[item.index] = identity
        self.source_groups.setdefault(identity, []).append(item)
        bound = bind_destination(destination)
        item.destination = bound
        item.phase = ItemPhase.INVENTORIED
        key = (bound.directory_identity, bound.basename)
        self.destination_groups.setdefault(key, []).append(item)

    def mark_collisions(self) -> None:
        for group in self.source_groups.values():
            if len(group) > 1:
                _mark_collision_group(
                    group,
                    AppError(
                        ExitCode.INPUT_ERROR,
                        "selected source aliases another input",
                    ),
                )
        for group in self.destination_groups.values():
            if len(group) > 1:
                _mark_collision_group(
                    group,
                    AppError(
                        ExitCode.OUTPUT_CONFLICT,
                        "two inputs target one destination",
                    ),
                )


def _destination_for(source: str, options: BatchOptions) -> str:
    """Compute output intent without normalizing the source path.

    Returns:
        The exact requested output intent before native destination binding.

    """
    if options.output is not None:
        return options.output
    return fitted_default_destination(source, options.output_dir)


def _mark(item: LedgerItem, error: AppError) -> None:
    if item.status is None:
        item.finish(ItemStatus.FAILED, error)


def _cancel_active_item(
    item: LedgerItem, ledger: BatchLedger, signal: str, phase: str
) -> None:
    if item.status is None:
        item.finish(
            ItemStatus.CANCELLED,
            AppError(ExitCode.INTERRUPTED, f"interrupted by {signal}", phase=phase),
        )
    ledger.record_interruption(signal, phase)


def _inventory(
    ledger: BatchLedger, sources: list[str], options: BatchOptions
) -> _Inventory | None:
    inventory = _Inventory.empty()
    for item, source in zip(ledger.items, sources, strict=True):
        if _inventory_item(item, source, options, inventory, ledger):
            return None
    inventory.mark_collisions()
    return inventory


def _inventory_item(
    item: LedgerItem,
    source: str,
    options: BatchOptions,
    inventory: _Inventory,
    ledger: BatchLedger,
) -> bool:
    try:
        checkpoint()
        inventory.add(item, source, options)
        checkpoint()
    except AppError as exc:
        _mark(item, exc)
    except CancellationSignal as cancellation:
        _cancel_active_item(item, ledger, cancellation.name, item.phase.value)
        return True
    except KeyboardInterrupt:
        _cancel_active_item(item, ledger, "SIGINT", item.phase.value)
        return True
    except SystemExit:
        _internal_abort(item, ledger, "unexpected SystemExit", "inventory")
        return True
    except MemoryError:
        raise
    except Exception as exc:  # ruff: ignore[blind-except] - batch ledger owns unknown failures.
        _internal_abort(item, ledger, str(exc) or type(exc).__name__, "inventory")
        return True
    return False


def _mark_collision_group(group: list[LedgerItem], error: AppError) -> None:
    for item in group:
        _mark(item, error)


def _candidate(item: LedgerItem, expected_identity: FileIdentity) -> None:
    """Bind a source, plan raw-span deletions, and verify the candidate once.

    Raises:
        AppError: If source binding, MIME planning, or candidate verification fails.

    """
    source_text = item.source_request.text
    if source_text is None:
        raise AppError(ExitCode.INPUT_ERROR, MISSING_SOURCE_ADDRESS)
    snapshot = read_source(source_text)
    checkpoint()
    if snapshot.identity != expected_identity:
        raise AppError(ExitCode.INPUT_ERROR, "source changed after inventory")
    item.source = snapshot
    item.phase = ItemPhase.BOUND
    tree = parse_raw_mime(snapshot.raw)
    checkpoint()
    item.phase = ItemPhase.PARSED
    policy = classify(tree.root)
    checkpoint()
    item.phase = ItemPhase.CLASSIFIED
    candidate = build_candidate(tree, policy.removals)
    receipt, independently_recomputed = verify_candidate(
        tree, candidate, policy.removals
    )
    checkpoint()
    item.transformation = TransformationPlan(
        policy.removals,
        independently_recomputed,
        candidate.stripped_headers,
        candidate.digest,
        len(candidate.raw),
        candidate.raw,
    )
    for fingerprint in independently_recomputed:
        for name, value in fingerprint.content_type_parameters:
            if name == b"charset":
                item.warnings.append({
                    "code": "CHARSET_PRESERVED_OPAQUE",
                    "mime_path": fingerprint.source_path,
                    "charset_base64": b64encode(value).decode(),
                    "message": "charset label was preserved without codec lookup",
                })
    item.verification = receipt
    if policy.removals and any(
        removal.reason.value == "RELATED_NONROOT_COMPONENT"
        for removal in policy.removals
    ):
        item.warnings.append({
            "code": "RELATED_REFERENCES_MAY_BE_UNRESOLVED",
            "message": "retained HTML may reference removed related components",
        })
    item.phase = ItemPhase.CANDIDATE


def _existing_or_publish(
    item: LedgerItem,
    options: BatchOptions,
    source_identities: set[FileIdentity],
) -> BaseException | None:
    destination, plan = _publication_inputs(item)
    if _verify_existing(item, destination, plan, options, source_identities):
        return None
    if options.dry_run:
        item.publication = PublicationReceipt(
            visibility="not_attempted",
            identity=None,
            digest=None,
            file_sync="not_attempted",
            directory_sync="not_attempted",
            address_verified=False,
            final_address=None,
            temp_cleanup="not_attempted",
        )
        item.finish(ItemStatus.WOULD_CREATE)
        return None
    item.phase = ItemPhase.STAGED
    checkpoint()
    try:
        item.publication = publish(destination, plan.candidate)
    except PublishedWithError as exc:
        item.publication = exc.receipt
        item.finish(ItemStatus.PUBLISHED_WITH_ERROR, _publication_error(exc.cause))
        return exc.cause
    item.phase = ItemPhase.PUBLISHED
    item.finish(ItemStatus.CREATED)
    return None


def _publication_inputs(
    item: LedgerItem,
) -> tuple[BoundDestination, TransformationPlan]:
    if item.destination is None or item.transformation is None:
        raise AppError(ExitCode.INTERNAL_ERROR, MISSING_PUBLICATION_INPUTS)
    return item.destination, item.transformation


def _verify_existing(
    item: LedgerItem,
    destination: BoundDestination,
    plan: TransformationPlan,
    options: BatchOptions,
    source_identities: set[FileIdentity],
) -> bool:
    if options.existing == "error":
        if existing_identity(destination) is not None:
            raise AppError(ExitCode.OUTPUT_CONFLICT, "destination already exists")
        return False
    existing = read_existing(destination)
    if existing is None:
        return False
    if existing.identity in source_identities:
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "existing output aliases a selected source"
        )
    if existing.raw != plan.candidate:
        raise AppError(
            ExitCode.OUTPUT_CONFLICT,
            "existing output is not the exact current candidate",
        )
    if existing.final_address is None:
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "could not prove existing output address"
        )
    item.publication = PublicationReceipt(
        visibility="existing_verified",
        identity=existing.identity,
        digest=plan.candidate_sha256,
        file_sync="not_attempted",
        directory_sync="not_attempted",
        address_verified=True,
        final_address=existing.final_address,
        temp_cleanup="not_applicable",
    )
    item.finish(ItemStatus.EXISTING_VERIFIED)
    return True


def _internal_abort(
    item: LedgerItem,
    ledger: BatchLedger,
    message: str,
    phase: str = "internal",
) -> None:
    error = AppError(ExitCode.INTERNAL_ERROR, message, phase=phase)
    _mark(item, error)
    ledger.batch_error = error
    ledger.finalize_not_run("not run after internal abort")


def execute(
    sources: list[str],
    options: BatchOptions,
    *,
    retain_evidence: bool = False,
    ledger: BatchLedger | None = None,
) -> BatchLedger:
    """Process every input in order while preserving every terminal ledger record.

    Returns:
        The fully terminalized authoritative ledger for this invocation.

    """
    with ExitStack() as storage:
        if ledger is None:
            ledger = BatchLedger.from_requests([
                path_value(source) for source in sources
            ])
            storage.callback(report_stream.close, ledger)
        ledger.retain_evidence = retain_evidence
        result = batch_execution.run(
            ledger,
            sources,
            options,
            _run_inventory_and_items,
            limits=(MAX_BATCH_ITEMS, MAX_CUMULATIVE_NATIVE_ARGUMENT_BYTES),
        )
        storage.pop_all()
        return result


def _run_inventory_and_items(
    ledger: BatchLedger, sources: list[str], options: BatchOptions
) -> None:
    inventory = _inventory(ledger, sources, options)
    if inventory is None:
        return
    budget = plan(ledger)
    if budget is None:
        report_stream.recover(ledger)
        return
    ledger.report_budget = budget
    if not report_stream.archive_or_recover(ledger):
        return
    all_identities = set(inventory.identities.values())
    for item in ledger.items:
        if batch_terminal.skip_inventory_failure(
            item, ledger, fail_fast=options.fail_fast
        ):
            return
        if item.status is not None:
            continue
        if _run_item(item, ledger, inventory.identities, all_identities, options):
            return


def _run_item(
    item: LedgerItem,
    ledger: BatchLedger,
    identities: dict[int, FileIdentity],
    all_identities: set[FileIdentity],
    options: BatchOptions,
) -> bool:
    publication_cause: BaseException | None = None
    try:
        if batch_execution.prepare_candidate(item, identities[item.index], _candidate):
            admit(ledger, item)
            with coherent_operation():
                publication_cause = _existing_or_publish(item, options, all_identities)
        checkpoint()
    except AppError as exc:
        _mark(item, exc)
        if exc.code is ExitCode.INTERNAL_ERROR:
            _internal_abort(item, ledger, exc.message, exc.phase or "internal")
            return True
        return _after_item(item, ledger, options, None)
    except (CancellationSignal, KeyboardInterrupt) as cancellation:
        _cancel_active_item(
            item,
            ledger,
            cancellation.name
            if isinstance(cancellation, CancellationSignal)
            else "SIGINT",
            item.phase.value,
        )
        return True
    except SystemExit:
        _internal_abort(item, ledger, "unexpected SystemExit")
        return True
    except MemoryError:
        raise
    except Exception as exc:  # ruff: ignore[blind-except] - batch ledger owns unknown failures.
        _internal_abort(item, ledger, str(exc) or type(exc).__name__)
        return True
    return _after_item(item, ledger, options, publication_cause)


def _after_item(
    item: LedgerItem,
    ledger: BatchLedger,
    options: BatchOptions,
    publication_cause: BaseException | None,
) -> bool:
    selected = batch_terminal.after_item(
        item,
        ledger,
        fail_fast=options.fail_fast,
        publication_cause=publication_cause,
        internal_abort=lambda message: _internal_abort(item, ledger, message),
    )
    checkpoint()
    return selected

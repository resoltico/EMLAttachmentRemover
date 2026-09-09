"""Exact receipts for batch terminalization and public command entry points."""

from __future__ import annotations

import pytest

from eml_attachment_remover import batch, cli, processing
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    BoundDestination,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    TransformationPlan,
)


def _options(*, dry_run: bool = False, fail_fast: bool = False) -> BatchOptions:
    """Build stable batch execution options.

    Returns:
        One option bundle for direct batch-control tests.

    """
    return BatchOptions(
        dry_run=dry_run,
        existing="error",
        fail_fast=fail_fast,
        output=None,
        output_dir=None,
    )


def _ledger() -> tuple[BatchLedger, LedgerItem, LedgerItem]:
    """Allocate two direct ledger rows.

    Returns:
        The ledger and its active and later rows.

    """
    ledger = BatchLedger.from_requests([
        PathValue("one", "one", "b25l"),
        PathValue("two", "two", "dHdv"),
    ])
    return ledger, ledger.items[0], ledger.items[1]


def test_fail_fast_skip_terminalizes_later_rows_with_the_exact_reason() -> None:
    """An inventory failure halts later work only when fail-fast was selected."""
    ledger, item, later = _ledger()
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad MIME"))

    assert batch._skip_inventory_failure(  # ruff: ignore[private-member-access] - direct fail-fast boundary.
        item, ledger, _options(fail_fast=True)
    )
    assert later.status is ItemStatus.NOT_RUN
    assert later.error == AppError(
        ExitCode.BATCH_FAILURE, "not run after fail-fast failure", phase="batch"
    )

    ledger, item, later = _ledger()
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad MIME"))
    assert not batch._skip_inventory_failure(  # ruff: ignore[private-member-access] - non-fail-fast continuation.
        item, ledger, _options(fail_fast=False)
    )
    assert later.status is None


def test_run_item_assigns_the_default_internal_phase_exactly(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An internal item fault without phase gets the fixed internal phase receipt."""
    ledger, item, later = _ledger()
    identity = FileIdentity(1, 2, "regular", 3)
    failure = AppError(ExitCode.INTERNAL_ERROR, "invariant")
    monkeypatch.setattr(
        batch,
        "_candidate",
        lambda *_arguments: (_ for _ in ()).throw(failure),
    )

    assert batch._run_item(  # ruff: ignore[private-member-access] - unphased internal item boundary.
        item, ledger, {0: identity}, {identity}, _options()
    )
    assert item.error is failure
    assert ledger.batch_error == AppError(
        ExitCode.INTERNAL_ERROR, "invariant", phase="internal"
    )
    assert later.error == AppError(
        ExitCode.BATCH_FAILURE, "not run after internal abort", phase="batch"
    )


def test_inventory_requires_exactly_one_source_per_preallocated_ledger_row(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Inventory refuses truncated input rather than silently ignoring a ledger row."""
    ledger = BatchLedger.from_requests([
        PathValue("one", "one", "b25l"),
        PathValue("two", "two", "dHdv"),
    ])
    monkeypatch.setattr(batch, "_inventory_item", lambda *_arguments: False)

    with pytest.raises(
        ValueError, match=r"zip\(\) argument 2 is shorter than argument 1"
    ):
        batch._inventory(  # ruff: ignore[private-member-access] - strict input-to-ledger pairing.
            ledger, ["one"], _options()
        )


def test_inventory_continues_after_a_non_fail_fast_inventory_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A prefailed row is skipped while every later eligible row is still run."""
    ledger = BatchLedger.from_requests([
        PathValue("zero", "zero", "emVybw=="),
        PathValue("one", "one", "b25l"),
        PathValue("two", "two", "dHdv"),
    ])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.INPUT_ERROR, "bad"))
    identity = FileIdentity(1, 2, "regular", 3)
    inventory = batch._Inventory(  # ruff: ignore[private-member-access] - direct planned inventory.
        {1: identity, 2: identity}, {}, {}
    )
    called: list[int] = []
    monkeypatch.setattr(batch, "_inventory", lambda *_arguments: inventory)

    def run(
        item: LedgerItem,
        _ledger: BatchLedger,
        _identities: dict[int, FileIdentity],
        _all: set[FileIdentity],
        _options: BatchOptions,
    ) -> bool:
        called.append(item.index)
        return False

    monkeypatch.setattr(batch, "_run_item", run)
    batch._run_inventory_and_items(  # ruff: ignore[private-member-access] - row-continuation boundary.
        ledger, ["zero", "one", "two"], _options(fail_fast=False)
    )
    assert called == [1, 2]


def test_existing_publication_enters_staged_phase_before_native_visibility(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Non-dry publication marks staging before passing bytes to the native edge."""
    item = LedgerItem(0, PathValue("source", "source", "c291cmNl"))
    destination = BoundDestination(
        PathValue("output", "output", "b3V0cHV0"),
        PathValue("parent", "parent", "cGFyZW50"),
        b"output.eml",
        FileIdentity(1, 2, "directory", 3),
    )
    plan = TransformationPlan((), (), (), "digest", 9, b"candidate")
    receipt = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(4, 5, "regular", 6),
        digest="digest",
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=destination.request,
        temp_cleanup="succeeded",
    )
    monkeypatch.setattr(batch, "_publication_inputs", lambda _item: (destination, plan))
    monkeypatch.setattr(batch, "_verify_existing", lambda *_arguments: False)

    def publish(
        observed_destination: BoundDestination, candidate: bytes
    ) -> PublicationReceipt:
        assert observed_destination == destination
        assert candidate == b"candidate"
        assert item.phase is ItemPhase.STAGED
        return receipt

    monkeypatch.setattr(batch, "publish", publish)
    assert (
        batch._existing_or_publish(  # ruff: ignore[private-member-access] - previsibility staged phase.
            item, _options(dry_run=False), set()
        )
        is None
    )
    assert item.phase is ItemPhase.PUBLISHED
    assert item.status is ItemStatus.CREATED
    assert item.publication == receipt


def test_publication_inputs_names_the_missing_facts_exactly() -> None:
    """Missing destination or transformation is an explicit internal receipt."""
    item = LedgerItem(0, PathValue("source", "source", "c291cmNl"))
    with pytest.raises(AppError) as raised:
        batch._publication_inputs(item)  # ruff: ignore[private-member-access] - publication fact prerequisite.
    assert raised.value == AppError(
        ExitCode.INTERNAL_ERROR, "candidate publication lacks destination or plan"
    )


def test_cli_interruption_detection_accepts_each_independent_terminal_evidence() -> (
    None
):
    """Cancellation and post-edge interruption each force the interruption outcome."""
    ledger, item, _later = _ledger()
    item.finish(ItemStatus.CANCELLED, AppError(ExitCode.INTERRUPTED, "cancelled"))
    assert cli._is_interrupted(ledger)  # ruff: ignore[private-member-access] - cancelled ledger receipt.

    ledger, item, _later = _ledger()
    item.finish(
        ItemStatus.PUBLISHED_WITH_ERROR,
        AppError(ExitCode.INTERRUPTED, "interrupted after publication"),
    )
    assert cli._is_interrupted(ledger)  # ruff: ignore[private-member-access] - post-edge interruption receipt.


def test_cli_selects_exact_modes_and_channels_and_main_preserves_argv(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Report mode/channel selection and argv handling retain all user intent."""
    ledger, _item, _later = _ledger()
    document: dict[str, object] = {"items": []}
    modes: list[str] = []
    channels: list[tuple[str, object]] = []

    def report(_ledger: BatchLedger, mode: str, _status: int) -> dict[str, object]:
        modes.append(mode)
        return document

    monkeypatch.setattr(cli, "report", report)
    monkeypatch.setattr(
        cli, "write_json", lambda value: channels.append(("json", value))
    )
    monkeypatch.setattr(
        cli, "write_human", lambda value: channels.append(("human", value))
    )
    cli._write_selected(  # ruff: ignore[private-member-access] - apply JSON selection.
        "json", ledger, _options(dry_run=False), 7
    )
    cli._write_selected(  # ruff: ignore[private-member-access] - dry human selection.
        "human", ledger, _options(dry_run=True), 0
    )
    assert modes == ["apply", "dry-run"]
    assert channels == [("json", document), ("human", document)]

    dispatched: list[list[str]] = []
    monkeypatch.setattr(
        cli.__dict__["sys"], "argv", ["program", "--dry-run", "one.eml"]
    )

    def dispatch(raw: list[str]) -> int:
        dispatched.append(raw)
        return 17

    monkeypatch.setattr(cli, "_dispatch", dispatch)
    assert cli.main() == 17
    assert dispatched == [["--dry-run", "one.eml"]]


def test_process_file_forwards_the_complete_default_and_custom_option_bundle(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The single-file facade does not alter caller-selected batch semantics."""
    observed: list[tuple[list[str], BatchOptions]] = []
    expected = BatchLedger.from_requests([PathValue("source", "source", "c291cmNl")])

    def execute(sources: list[str], options: BatchOptions) -> BatchLedger:
        observed.append((sources, options))
        return expected

    monkeypatch.setattr(processing, "execute", execute)
    assert processing.process_file("source") is expected
    assert (
        processing.process_file("source", "copy.eml", existing="verify", dry_run=True)
        is expected
    )
    assert observed == [
        (
            ["source"],
            BatchOptions(
                dry_run=False,
                existing="error",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
        ),
        (
            ["source"],
            BatchOptions(
                dry_run=True,
                existing="verify",
                fail_fast=False,
                output="copy.eml",
                output_dir=None,
            ),
        ),
    ]

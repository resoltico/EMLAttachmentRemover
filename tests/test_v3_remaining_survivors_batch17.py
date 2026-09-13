"""Exact behavioral receipts for the final non-parser mutation survivors."""

from __future__ import annotations

import importlib
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

from eml_attachment_remover import batch, mime_execution, native_windows_binding
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.domain import BatchLedger, FileIdentity
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from collections.abc import Callable

    import pytest

    from eml_attachment_remover.domain import LedgerItem


def _options() -> BatchOptions:
    """Build a policy-bearing options object for one post-item handoff.

    Returns:
        Options whose fail-fast policy must reach post-item terminalization.

    """
    return BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=True,
        output=None,
        output_dir=None,
    )


def test_resolved_version_requests_the_named_distribution(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Installed metadata lookup uses the one public distribution identity."""
    version_module = importlib.import_module("eml_attachment_remover._version")
    resolved = cast("Callable[[], str]", version_module.__dict__["_resolved_version"])
    requested: list[str | None] = []

    def distribution_version(name: str | None) -> str:
        requested.append(name)
        return "3.0.0"

    monkeypatch.setitem(
        version_module.__dict__, "distribution_version", distribution_version
    )
    assert resolved() == "3.0.0"
    assert requested == [version_module.DISTRIBUTION_NAME]


def test_run_item_forwards_its_complete_options_to_post_item_policy(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Post-item policy receives the invocation options selected by the caller."""
    ledger = BatchLedger.from_requests([path_value("one.eml")])
    item = ledger.items[0]
    identity = FileIdentity(1, 2, "regular", 3)
    options = _options()
    observed: list[BatchOptions] = []

    monkeypatch.setattr(batch, "_candidate", lambda *_arguments: None)
    monkeypatch.setattr(batch, "_existing_or_publish", lambda *_arguments: None)

    def after_item(
        _item: LedgerItem,
        _ledger: BatchLedger,
        received: BatchOptions,
        _publication_cause: BaseException | None,
    ) -> bool:
        observed.append(received)
        return False

    monkeypatch.setattr(batch, "_after_item", after_item)
    assert not batch._run_item(  # ruff: ignore[private-member-access] - policy handoff.
        item, ledger, {0: identity}, {identity}, options
    )
    assert observed == [options]


def test_apply_deletes_indexed_spans_and_leaves_an_empty_plan_byte_identical() -> None:
    """Raw deletion has no implicit offset and treats no edits as the identity."""
    raw = b"0123456789"
    assert mime_execution._apply(raw, []) == raw  # ruff: ignore[private-member-access] - direct byte-span execution.
    assert (
        mime_execution._apply(  # ruff: ignore[private-member-access] - direct byte-span execution.
            raw, [(1, 3), (5, 7)]
        )
        == b"034789"
    )


@dataclass
class _ParentApi:
    """Record each parent open without requiring the Windows runtime."""

    opened: list[tuple[str, int | None]] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        """Record one descriptor-rooted parent request.

        Returns:
            A deterministic owned parent-directory handle.

        """
        self.opened.append((path, root))
        return 41


def test_windows_unc_parent_spellings_bypass_the_captured_working_directory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Both legal UNC separator spellings are independently absolute."""
    api = _ParentApi()
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)

    assert native_windows_binding._parent(r"\\server\share\mail.eml") == (  # ruff: ignore[private-member-access] - backslash UNC root behavior.
        41,
        "\\\\server\\share\\",
        "mail.eml",
    )
    assert native_windows_binding._parent("//server/share/mail.eml") == (  # ruff: ignore[private-member-access] - slash UNC root behavior.
        41,
        "//server/share/",
        "mail.eml",
    )
    assert api.opened == [
        ("\\\\server\\share\\", None),
        ("//server/share/", None),
    ]

"""Receipt-construction fault contracts for v3 publication staging."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import staged_output
from eml_attachment_remover.domain import AppError
from eml_attachment_remover.native_paths import (
    bind_destination,
    publish_stage_no_replace,
)
from eml_attachment_remover.staged_output import (
    _PublicationState,  # ruff: ignore[import-private-name] - direct receipt-state fault contract.
)

if TYPE_CHECKING:
    from pathlib import Path


def test_final_receipt_defensively_rejects_missing_constructor_result(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = bind_destination(str(tmp_path / "output.eml"))
    candidate = b"candidate"
    state = _PublicationState(
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - direct receipt fault setup.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - direct receipt fault setup.
    staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - direct receipt fault setup.
    stage = state.stage
    parent = state.parent
    assert stage is not None
    assert parent is not None
    assert stage.name is not None
    publish_stage_no_replace(
        parent, stage.descriptor, stage.name, state.destination.basename
    )
    state.kernel_published = True
    with monkeypatch.context() as context:
        context.setattr(staged_output, "PublicationReceipt", lambda **_kwargs: None)
        with pytest.raises(AppError):
            staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - missing-receipt defensive contract.
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.

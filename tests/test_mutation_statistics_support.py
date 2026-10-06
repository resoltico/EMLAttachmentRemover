"""Source sealing stays mandatory without becoming every test's mutation subject."""

from __future__ import annotations

from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest
from tools import mutation_integrity

from tests import mutation_statistics_support

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("fails", [False, True])
def test_workspace_capture_preserves_real_calls_and_propagates_its_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, fails: bool
) -> None:
    calls = {"processor.tested_call"}
    source = tmp_path / "workspace"
    checkpoint = tmp_path / "checkpoint.json"
    failure = RuntimeError("source inputs changed")
    observed: list[tuple[Path, Path]] = []

    def capture(workspace: Path, target: Path) -> None:
        observed.append((workspace, target))
        calls.update({"checkpoint.capture", "checkpoint.inventory"})
        if fails:
            raise failure
        target.write_text("sealed inputs", encoding="utf-8")

    monkeypatch.setattr(
        mutation_statistics_support,
        "mutation_state",
        lambda: SimpleNamespace(_stats=calls),
    )
    monkeypatch.setattr(mutation_integrity, "capture", capture)
    if fails:
        with pytest.raises(RuntimeError, match="source inputs changed") as caught:
            mutation_statistics_support.capture_workspace(source, checkpoint)
        assert caught.value is failure
        assert not checkpoint.exists()
    else:
        mutation_statistics_support.capture_workspace(source, checkpoint)
        assert checkpoint.read_text(encoding="utf-8") == "sealed inputs"
    assert calls == {"processor.tested_call"}
    assert observed == [(source, checkpoint)]

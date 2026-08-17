"""Stateful property tests for non-destructive filesystem transitions."""

from __future__ import annotations

import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import Final, Literal

import pytest
from hypothesis import event
from hypothesis import strategies as st
from hypothesis.stateful import RuleBasedStateMachine, invariant, rule

from eml_attachment_remover import process_file
from eml_attachment_remover.models import CliError, ExitCode
from tests.test_support import parse

type OutputSlot = Literal["default", "explicit"]
SLOTS: Final[st.SearchStrategy[OutputSlot]] = st.sampled_from(("default", "explicit"))
SENTINEL: Final = b"public preexisting output"


def _source_bytes() -> bytes:
    """Build one stable public source used throughout a state-machine run.

    Returns:
        A MIME message with one body and one removable attachment.

    """
    message = EmailMessage()
    message["Subject"] = "Stateful public fixture"
    message.set_content("Stateful body must remain")
    message.add_attachment(
        b"stateful attachment",
        maintype="application",
        subtype="octet-stream",
        filename="stateful.bin",
    )
    return message.as_bytes(policy=policy.SMTP)


class OutputStateMachine(RuleBasedStateMachine):
    """Model output creation, conflict, replacement, and dry-run transitions."""

    def __init__(self) -> None:
        """Create an isolated filesystem and exact expected output state."""
        super().__init__()
        self.temporary = tempfile.TemporaryDirectory()
        root = Path(self.temporary.name)
        self.source = root / "source.eml"
        self.source_raw = _source_bytes()
        self.source.write_bytes(self.source_raw)
        self.outputs: dict[OutputSlot, Path] = {
            "default": root / "source.text-only.eml",
            "explicit": root / "explicit-output.eml",
        }
        self.expected: dict[OutputSlot, bytes | None] = {
            "default": None,
            "explicit": None,
        }

    def _argument(self, slot: OutputSlot) -> Path | None:
        """Return the public API destination argument for one modeled slot.

        Returns:
            ``None`` for default naming, otherwise the explicit destination.

        """
        return None if slot == "default" else self.outputs[slot]

    def _record_output(self, slot: OutputSlot) -> None:
        """Validate a successful output and update the exact state model."""
        output = self.outputs[slot]
        parsed = parse(output)
        self.expected[slot] = output.read_bytes()
        body = parsed.get_body()
        assert body is not None
        assert body.get_content().strip() == "Stateful body must remain"
        assert not any(
            part.get_content_disposition() == "attachment" for part in parsed.walk()
        )

    @rule(slot=SLOTS)
    def create_external_output(self, slot: OutputSlot) -> None:
        """Model another actor creating or replacing an output path."""
        self.outputs[slot].write_bytes(SENTINEL)
        self.expected[slot] = SENTINEL
        event(f"stateful-external-write={slot}")

    @rule(slot=SLOTS)
    def remove_output(self, slot: OutputSlot) -> None:
        """Model an output being removed between processing attempts."""
        self.outputs[slot].unlink(missing_ok=True)
        self.expected[slot] = None
        event(f"stateful-remove={slot}")

    @rule(slot=SLOTS)
    def process_without_force(self, slot: OutputSlot) -> None:
        """Process once and model success or a non-destructive conflict."""
        before = self.expected[slot]
        if before is None:
            result = process_file(
                self.source,
                self._argument(slot),
                force=False,
                dry_run=False,
            )
            assert [part.filename for part in result.removed_attachments] == [
                "stateful.bin"
            ]
            self._record_output(slot)
            event(f"stateful-create={slot}")
            return
        with pytest.raises(CliError) as raised:
            process_file(
                self.source,
                self._argument(slot),
                force=False,
                dry_run=False,
            )
        assert raised.value.code == ExitCode.OUTPUT_CONFLICT
        assert self.outputs[slot].read_bytes() == before
        event(f"stateful-conflict={slot}")

    @rule(slot=SLOTS)
    def process_with_force(self, slot: OutputSlot) -> None:
        """Process while allowing safe replacement of an existing output."""
        result = process_file(
            self.source,
            self._argument(slot),
            force=True,
            dry_run=False,
        )
        assert [part.filename for part in result.removed_attachments] == [
            "stateful.bin"
        ]
        self._record_output(slot)
        event(f"stateful-force={slot}")

    @rule(slot=SLOTS)
    def dry_run_preserves_every_path(self, slot: OutputSlot) -> None:
        """Ensure dry-run processing never mutates source or destinations."""
        before = self.expected[slot]
        result = process_file(
            self.source,
            self._argument(slot),
            force=False,
            dry_run=True,
        )
        assert result.dry_run
        assert result.destination is None
        assert [part.filename for part in result.removed_attachments] == [
            "stateful.bin"
        ]
        actual = (
            self.outputs[slot].read_bytes() if self.outputs[slot].exists() else None
        )
        assert actual == before
        event(f"stateful-dry-run={slot}")

    @rule()
    def reject_direct_source_alias(self) -> None:
        """Ensure force cannot authorize direct source replacement."""
        with pytest.raises(CliError) as raised:
            process_file(self.source, self.source, force=True, dry_run=False)
        assert raised.value.code == ExitCode.OUTPUT_CONFLICT
        event("stateful-source-alias-rejected")

    @invariant()
    def source_and_modeled_outputs_remain_exact(self) -> None:
        """Check source immutability, output state, and temporary-file cleanup."""
        assert self.source.read_bytes() == self.source_raw
        for slot, output in self.outputs.items():
            actual = output.read_bytes() if output.exists() else None
            assert actual == self.expected[slot]
        assert list(self.source.parent.glob(".eml-remove-*.tmp")) == []

    def teardown(self) -> None:
        """Release the isolated filesystem after each generated sequence."""
        self.temporary.cleanup()


OutputStateMachineTest = OutputStateMachine.TestCase

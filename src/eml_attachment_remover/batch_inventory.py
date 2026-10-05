"""Bind input metadata and detect source/destination collisions before processing."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from .destination_names import fitted_default_destination
from .domain import AppError, ExitCode, FileIdentity, ItemPhase, ItemStatus, LedgerItem
from .native_paths import (
    bind_destination,
    inspect_source,
    inspect_source_identity,
    path_value,
)

if TYPE_CHECKING:
    from .batch import BatchOptions


@dataclass(slots=True)
class Inventory:
    """Bounded, file-metadata-only facts retained between batch phases."""

    identities: dict[int, FileIdentity]
    source_groups: dict[FileIdentity, list[LedgerItem]]
    destination_groups: dict[tuple[FileIdentity, bytes | str], list[LedgerItem]]

    @classmethod
    def empty(cls) -> Inventory:
        """Start without retained source or destination bindings.

        Returns:
            An empty planning inventory.

        """
        return cls({}, {}, {})

    def add(self, item: LedgerItem, source: str, options: BatchOptions) -> None:
        """Bind one request and retain its small planning metadata.

        Raises:
            AppError: If automatic naming lacks an opened-source address.

        """
        if options.output_dir is not None and options.output is None:
            identity, address = inspect_source(source)
            if address is None or address.text is None:
                raise AppError(
                    ExitCode.INPUT_ERROR,
                    "source address is unavailable for automatic output naming",
                )
            destination = fitted_default_destination(address.text, options.output_dir)
        else:
            destination = _destination_for(source, options)
            identity = inspect_source_identity(source)
        item.destination_request = path_value(destination)
        self.identities[item.index] = identity
        self.source_groups.setdefault(identity, []).append(item)
        bound = bind_destination(destination)
        item.destination = bound
        item.phase = ItemPhase.INVENTORIED
        key = (bound.directory_identity, bound.basename)
        self.destination_groups.setdefault(key, []).append(item)

    def mark_collisions(self) -> None:
        """Refuse every colliding request before processing any candidate."""
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


def _mark_collision_group(group: list[LedgerItem], error: AppError) -> None:
    for item in group:
        if item.status is None:
            item.finish(ItemStatus.FAILED, error)

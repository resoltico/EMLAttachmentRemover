"""Public native-path API composed from cohesive value and binding modules."""

from __future__ import annotations

from .domain import BoundDirectory as BoundDirectoryHandle
from .native_binding import (
    bind_destination,
    child_lstat,
    close_bound_directory,
    create_private_stage,
    descriptor_identity,
    discard_private_stage,
    existing_identity,
    inspect_source_identity,
    open_bound_destination,
    open_child_nofollow,
    private_stage_name,
    publish_stage_no_replace,
    read_existing,
    read_source,
    sync_bound_directory,
)
from .native_values import (
    MAX_PATH_BYTES,
    MAX_RAW_BYTES,
    default_destination,
    path_value,
    require_native_backend,
    validate_argument,
    validate_windows_argument,
)

__all__ = [
    "MAX_PATH_BYTES",
    "MAX_RAW_BYTES",
    "BoundDirectoryHandle",
    "bind_destination",
    "child_lstat",
    "close_bound_directory",
    "create_private_stage",
    "default_destination",
    "descriptor_identity",
    "discard_private_stage",
    "existing_identity",
    "inspect_source_identity",
    "open_bound_destination",
    "open_child_nofollow",
    "path_value",
    "private_stage_name",
    "publish_stage_no_replace",
    "read_existing",
    "read_source",
    "require_native_backend",
    "sync_bound_directory",
    "validate_argument",
    "validate_windows_argument",
]

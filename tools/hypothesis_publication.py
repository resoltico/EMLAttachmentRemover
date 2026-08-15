"""Publish sanitized Hypothesis observations from isolated storage."""

from __future__ import annotations

import importlib
import shutil
import stat
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from typing import Protocol

    class ArtifactSanitizer(Protocol):
        """Describe observation sanitization used before publication."""

        def finalize(
            self,
            root: Path,
            *,
            observations: bool = False,
            additional_replacements: tuple[tuple[Path, str], ...] = (),
        ) -> tuple[int, int]:
            """Sanitize one isolated observation set."""


artifacts = cast(
    "ArtifactSanitizer",
    importlib.import_module(
        "tools.finalize_hypothesis_artifacts"
        if __package__
        else "finalize_hypothesis_artifacts"
    ),
)
HYPOTHESIS_DIRECTORY: Final = ".hypothesis"
OBSERVATION_DIRECTORY: Final = "observed"
PROJECT_PLACEHOLDER: Final = "<project-root>"
PUBLIC_ROOT_LABEL: Final = "public Hypothesis root"
PUBLIC_OBSERVATIONS_LABEL: Final = "public Hypothesis observations"
NO_OBSERVATIONS_MESSAGE: Final = (
    "observable Hypothesis run produced no observation files"
)
STAGING_PREFIX: Final = ".observed-publication-"
STAGING_OBSERVATIONS_NAME: Final = "observed"
PREVIOUS_OBSERVATIONS_NAME: Final = "previous"
PUBLICATION_GROUP_MESSAGE: Final = "Hypothesis observation publication failed"


class HypothesisPublicationError(ValueError):
    """Report an unsafe or failed observation publication."""


def _directory_exists(path: Path, label: str) -> bool:
    """Return whether a path is a real directory, rejecting unsafe entries.

    Returns:
        Whether the directory exists.

    Raises:
        HypothesisPublicationError: If the entry cannot be inspected or is unsafe.

    """
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return False
    except OSError as error:
        message = f"cannot inspect {label} {path}: {error}"
        raise HypothesisPublicationError(message) from error
    if not stat.S_ISDIR(mode):
        message = f"{label} must be a real directory when present: {path}"
        raise HypothesisPublicationError(message)
    return True


def _discard_staging(root: Path, action_error: BaseException | None) -> None:
    """Remove one owned staging root and preserve an earlier failure.

    Raises:
        BaseExceptionGroup: If control flow and cleanup both fail.
        ExceptionGroup: If an ordinary operation and cleanup both fail.

    """
    previous = root / PREVIOUS_OBSERVATIONS_NAME
    if action_error is not None and previous.exists():
        return
    try:
        shutil.rmtree(root)
    except OSError as error:
        cleanup_error = HypothesisPublicationError(
            f"cannot remove Hypothesis observation staging directory {root}: {error}"
        )
        if action_error is None:
            raise cleanup_error from error
        if isinstance(action_error, Exception):
            raise ExceptionGroup(
                PUBLICATION_GROUP_MESSAGE,
                [action_error, cleanup_error],
            ) from None
        raise BaseExceptionGroup(
            PUBLICATION_GROUP_MESSAGE,
            [action_error, cleanup_error],
        ) from None


def _stage_observations(source: Path, hypothesis_root: Path) -> tuple[Path, Path]:
    """Copy observations into an owned same-parent staging root.

    Returns:
        The staging root and staged observation directory.

    Raises:
        HypothesisPublicationError: If staging cannot be created or populated.

    """
    try:
        root = Path(tempfile.mkdtemp(dir=hypothesis_root, prefix=STAGING_PREFIX))
    except OSError as error:
        message = f"cannot create Hypothesis observation staging directory: {error}"
        raise HypothesisPublicationError(message) from error
    staged = root / STAGING_OBSERVATIONS_NAME
    try:
        shutil.copytree(source, staged, copy_function=shutil.copyfile)
    except (OSError, UnicodeError) as error:
        failure = HypothesisPublicationError(
            f"cannot stage public Hypothesis observations: {error}"
        )
        _discard_staging(root, failure)
        raise failure from error
    except BaseException as error:
        _discard_staging(root, error)
        raise
    return root, staged


def _restore_previous(
    previous: Path,
    destination: Path,
    promotion_error: BaseException,
) -> None:
    """Restore the prior publication after a failed promotion.

    Raises:
        BaseExceptionGroup: If control flow and restoration both fail.
        ExceptionGroup: If ordinary promotion and restoration both fail.

    """
    if not previous.exists() or destination.exists() or destination.is_symlink():
        return
    try:
        previous.replace(destination)
    except OSError:
        restore_error = HypothesisPublicationError(
            "cannot restore prior Hypothesis observations; retained in staging "
            f"entry {previous.parent.name!r}/{previous.name!r}"
        )
        if isinstance(promotion_error, Exception):
            raise ExceptionGroup(
                PUBLICATION_GROUP_MESSAGE,
                [promotion_error, restore_error],
            ) from None
        raise BaseExceptionGroup(
            PUBLICATION_GROUP_MESSAGE,
            [promotion_error, restore_error],
        ) from None


def _promote_observations(
    staging_root: Path,
    staged: Path,
    destination: Path,
) -> None:
    """Promote a complete staged set while preserving the prior publication."""
    previous = staging_root / PREVIOUS_OBSERVATIONS_NAME
    destination_exists = _directory_exists(destination, PUBLIC_OBSERVATIONS_LABEL)
    if destination_exists:
        destination.replace(previous)
    try:
        staged.replace(destination)
    except BaseException as error:
        _restore_previous(previous, destination, error)
        raise


def publish_observations(source_root: Path, publication_root: Path) -> tuple[int, int]:
    """Sanitize isolated observations and replace the public observation set.

    Returns:
        The number of observation files and records published.

    Raises:
        HypothesisPublicationError: If publication is absent, unsafe, or fails.

    """
    source_root = source_root.resolve()
    publication_root = publication_root.resolve()
    files, records = artifacts.finalize(
        source_root,
        observations=True,
        additional_replacements=((publication_root, PROJECT_PLACEHOLDER),),
    )
    if files == 0:
        raise HypothesisPublicationError(NO_OBSERVATIONS_MESSAGE)
    source = source_root / HYPOTHESIS_DIRECTORY / OBSERVATION_DIRECTORY
    hypothesis_root = publication_root / HYPOTHESIS_DIRECTORY
    if not _directory_exists(hypothesis_root, PUBLIC_ROOT_LABEL):
        try:
            hypothesis_root.mkdir()
        except OSError as error:
            message = f"cannot create public Hypothesis root {hypothesis_root}: {error}"
            raise HypothesisPublicationError(message) from error
    destination = hypothesis_root / OBSERVATION_DIRECTORY
    staging_root, staged = _stage_observations(source, hypothesis_root)
    try:
        try:
            _promote_observations(staging_root, staged, destination)
        except OSError as error:
            message = f"cannot promote Hypothesis observations {destination}: {error}"
            raise HypothesisPublicationError(message) from error
    finally:
        _discard_staging(staging_root, sys.exception())
    return files, records

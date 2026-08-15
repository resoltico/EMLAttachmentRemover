"""Run Hypothesis with private ephemeral non-example storage."""

from __future__ import annotations

import importlib
import os
import shutil
import stat
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, Never, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence
    from typing import Protocol

    class ArtifactModule(Protocol):
        """Describe the isolated-artifact operations used by this runner."""

        def finalize(
            self,
            root: Path,
            *,
            observations: bool = False,
        ) -> tuple[int, int]:
            """Finalize one isolated artifact root."""

    class PublicationModule(Protocol):
        """Describe publication of sanitized isolated observations."""

        def publish_observations(
            self,
            source_root: Path,
            publication_root: Path,
        ) -> tuple[int, int]:
            """Publish one sanitized observation set."""

    class JunitReportModule(Protocol):
        """Describe isolated JUnit report publication."""

        REPORT_NAME: str

        def publish(
            self,
            source: Path,
            destination: Path,
            project_root: Path,
        ) -> Path:
            """Sanitize and atomically publish one report."""


artifacts = cast(
    "ArtifactModule",
    importlib.import_module(
        "tools.finalize_hypothesis_artifacts"
        if __package__
        else "finalize_hypothesis_artifacts"
    ),
)
publisher = cast(
    "PublicationModule",
    importlib.import_module(
        "tools.hypothesis_publication" if __package__ else "hypothesis_publication"
    ),
)
junit_report = cast(
    "JunitReportModule",
    importlib.import_module("tools.junit_report" if __package__ else "junit_report"),
)

STORAGE_ENVIRONMENT_VARIABLE: Final = "HYPOTHESIS_STORAGE_DIRECTORY"
STORAGE_PREFIX: Final = "eml-attachment-remover-hypothesis-"
STORAGE_DIRECTORY_NAME: Final = ".hypothesis"
FINALIZATION_GROUP_MESSAGE: Final = "Hypothesis run finalization failed"


class HypothesisRunError(RuntimeError):
    """Report unsafe storage or failed isolated-storage cleanup."""


def _create_storage(project_root: Path) -> tuple[Path, Path]:
    """Create a private OS-temporary root outside the repository.

    Returns:
        The cleanup root and exact Hypothesis storage directory.

    """
    cleanup_root = Path(tempfile.mkdtemp(prefix=STORAGE_PREFIX)).resolve()
    try:
        storage = _initialize_storage(cleanup_root, project_root)
    except BaseException:
        if _is_owned_temporary_root(cleanup_root, project_root):
            shutil.rmtree(cleanup_root, ignore_errors=True)
        raise
    return cleanup_root, storage


def _is_owned_temporary_root(path: Path, project_root: Path) -> bool:
    """Return whether an exact generated root is safe to remove.

    Returns:
        Whether the path has the canonical private temporary identity.

    """
    temporary_parent = Path(tempfile.gettempdir()).resolve()
    project = project_root.resolve()
    return (
        path.parent == temporary_parent
        and path.name.startswith(STORAGE_PREFIX)
        and path != project
        and not project.is_relative_to(path)
    )


def _initialize_storage(cleanup_root: Path, project_root: Path) -> Path:
    """Validate the temporary root and create its exact Hypothesis home.

    Returns:
        The created Hypothesis home.

    Raises:
        HypothesisRunError: If the selected root is inside the repository or unsafe.

    """
    mode = cleanup_root.lstat().st_mode
    unsafe_location = not _is_owned_temporary_root(cleanup_root, project_root)
    unsafe_mode = not stat.S_ISDIR(mode)
    private_mode = os.name == "nt" or stat.S_IMODE(mode) & 0o077 == 0
    if unsafe_location or unsafe_mode or not private_mode:
        message = f"Hypothesis temporary storage is unsafe: {cleanup_root}"
        raise HypothesisRunError(message)
    storage = cleanup_root / STORAGE_DIRECTORY_NAME
    storage.mkdir(mode=0o700)
    return storage


def _cleanup(cleanup_root: Path) -> None:
    """Remove the exact isolated root and reject incomplete cleanup.

    Raises:
        HypothesisRunError: If removal fails or leaves the root present.

    """
    try:
        shutil.rmtree(cleanup_root)
    except OSError as error:
        message = f"cannot remove Hypothesis temporary storage {cleanup_root}: {error}"
        raise HypothesisRunError(message) from error
    if cleanup_root.exists() or cleanup_root.is_symlink():
        message = f"Hypothesis temporary storage cleanup was incomplete: {cleanup_root}"
        raise HypothesisRunError(message)


def _finish(
    cleanup_root: Path,
    project_root: Path,
    *,
    observations: bool,
    report_destination: Path | None,
    action_error: BaseException | None,
) -> None:
    """Publish, unconditionally clean, and preserve concurrent failures."""
    try:
        _finalize_outputs(
            cleanup_root,
            project_root,
            observations=observations,
            report_destination=report_destination,
        )
    except BaseException as finalization_error:
        _cleanup_after_finalization(cleanup_root, action_error, finalization_error)
        if action_error is not None:
            _raise_failure_group([action_error, finalization_error])
        raise
    _cleanup_after_finalization(cleanup_root, action_error, None)


def _finalize_outputs(
    cleanup_root: Path,
    project_root: Path,
    *,
    observations: bool,
    report_destination: Path | None,
) -> None:
    """Publish the requested artifacts or raise their collected failures."""
    failures: list[Exception] = []
    _publish_report(cleanup_root, project_root, report_destination, failures)
    try:
        if observations:
            publisher.publish_observations(cleanup_root, project_root)
        else:
            artifacts.finalize(cleanup_root)
    except (OSError, RuntimeError, ValueError) as error:
        failures.append(error)
    if failures:
        if len(failures) == 1:
            raise failures[0]
        _raise_failure_group(failures)


def _cleanup_after_finalization(
    cleanup_root: Path,
    action_error: BaseException | None,
    finalization_error: BaseException | None,
) -> None:
    """Clean temporary storage and combine a concurrent cleanup failure."""
    try:
        _cleanup(cleanup_root)
    except BaseException as cleanup_error:
        prior = [
            error for error in (action_error, finalization_error) if error is not None
        ]
        if prior:
            _raise_failure_group([*prior, cleanup_error])
        raise


def _raise_failure_group(failures: Sequence[BaseException]) -> Never:
    """Raise the correctly typed group for multiple finalization failures.

    Raises:
        BaseExceptionGroup: If any retained failure is control flow.
        ExceptionGroup: If every retained failure is an ordinary exception.

    """
    ordinary = [failure for failure in failures if isinstance(failure, Exception)]
    if len(ordinary) == len(failures):
        raise ExceptionGroup(FINALIZATION_GROUP_MESSAGE, ordinary) from None
    raise BaseExceptionGroup(FINALIZATION_GROUP_MESSAGE, failures) from None


def _publish_report(
    cleanup_root: Path,
    project_root: Path,
    destination: Path | None,
    failures: list[Exception],
) -> None:
    """Publish an expected report and retain one operational failure."""
    if destination is None:
        return
    try:
        junit_report.publish(
            cleanup_root / junit_report.REPORT_NAME,
            destination,
            project_root,
        )
    except (OSError, RuntimeError, ValueError) as error:
        failures.append(error)


def run_isolated(
    action: Callable[[Path], None],
    project_root: Path,
    *,
    observations: bool = False,
    report_destination: Path | None = None,
) -> None:
    """Run an action with private storage and always finalize that storage."""
    cleanup_root, storage = _create_storage(project_root)
    try:
        action(storage)
    finally:
        _finish(
            cleanup_root,
            project_root,
            observations=observations,
            report_destination=report_destination,
            action_error=sys.exception(),
        )

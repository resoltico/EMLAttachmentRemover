from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Protocol

class RepositoryIssue(Protocol):
    path: Path
    message: str

class RepositoryAudit(Protocol):
    public_files: tuple[Path, ...]
    issues: tuple[RepositoryIssue, ...]
    def diagnostics(self, root: Path) -> tuple[str, ...]: ...

class RepositoryHygiene(Protocol):
    def audit_repository(self, root: Path) -> RepositoryAudit: ...

class HypothesisRunner(Protocol):
    STORAGE_ENVIRONMENT_VARIABLE: str
    def run_isolated(
        self,
        action: Callable[[Path], None],
        root: Path,
        *,
        observations: bool = False,
        report_destination: Path | None = None,
    ) -> None: ...

class TaskTimeout(Protocol):
    def positive_timeout(self, value: str) -> float: ...

class TaskTestCommands(Protocol):
    def pytest_command(
        self,
        executable: str,
        report: Path,
        *,
        coverage: bool,
    ) -> tuple[str, ...]: ...

class MutationTaskModule(Protocol):
    def mutation_paths(
        self,
        project_root: Path,
        build_directory: Path,
        statistics: Path,
        results: Path,
        equivalents: Path,
    ) -> object: ...
    def mutation_actions(
        self,
        coverage: Callable[[], None],
        cleanup: Callable[[], None],
        capture: Callable[[], None],
        run: Callable[..., None],
    ) -> object: ...
    def capture_results(
        self,
        paths: object,
        executable: str,
        environment: Mapping[str, str],
    ) -> None: ...
    def remove_workspace(self, paths: object) -> None: ...
    def run_mutation(self, paths: object, actions: object, executable: str) -> None: ...

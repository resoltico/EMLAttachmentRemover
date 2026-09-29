"""One mutation run owns a checkout's workspace and evidence at a time."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from textwrap import dedent
from typing import Final

import pytest
from tools import mutation_lease, tasks

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
HOLDER: Final = dedent(
    """
    import sys
    from pathlib import Path
    from tools import mutation_lease

    with mutation_lease.lease(Path(sys.argv[1])):
        print("held", flush=True)
        sys.stdin.readline()
    """
)


@pytest.mark.skipif(os.name == "nt", reason="mutation testing requires fork support")
def test_a_second_run_in_the_same_checkout_fails_fast_naming_the_holder(
    tmp_path: Path,
) -> None:
    """Contention is refused at once, with the holder's pid, and frees on exit."""
    build = tmp_path / "build"
    holder = subprocess.Popen(
        [sys.executable, "-c", HOLDER, str(build)],
        cwd=PROJECT_ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert holder.stdout is not None
        assert holder.stdout.readline() == "held\n"
        with (
            pytest.raises(RuntimeError, match=f"pid {holder.pid}\\)") as raised,
            mutation_lease.lease(build),
        ):
            pass
        assert "another mutation run owns this checkout" in str(raised.value)
        assert "use a separate checkout" in str(raised.value)
    finally:
        assert holder.stdin is not None
        holder.stdin.write("\n")
        holder.stdin.close()
        holder.wait(timeout=30)
        assert holder.stdout is not None
        holder.stdout.close()
    with mutation_lease.lease(build):
        assert (build / mutation_lease.LOCK_NAME).read_text() == f"{os.getpid()}\n"


@pytest.mark.skipif(os.name == "nt", reason="mutation testing requires fork support")
def test_a_stale_lock_file_does_not_block_and_a_missing_holder_is_unknown(
    tmp_path: Path,
) -> None:
    """The kernel releases a dead holder's lock; an unrecorded holder is 'unknown'."""
    build = tmp_path / "build"
    build.mkdir()
    (build / mutation_lease.LOCK_NAME).write_text("999999999\n", encoding="ascii")
    with mutation_lease.lease(build):
        assert (build / mutation_lease.LOCK_NAME).read_text() == f"{os.getpid()}\n"
    (build / mutation_lease.LOCK_NAME).write_text("", encoding="ascii")
    other = os.open(build / mutation_lease.LOCK_NAME, os.O_RDWR)
    try:
        fcntl = pytest.importorskip("fcntl")
        fcntl.flock(other, fcntl.LOCK_EX)
        with pytest.raises(RuntimeError, match="pid unknown"):
            with mutation_lease.lease(build):
                pass
    finally:
        os.close(other)


@pytest.mark.skipif(os.name == "nt", reason="mutation testing requires fork support")
def test_the_lease_is_released_when_the_block_fails(tmp_path: Path) -> None:
    """A failing campaign leaves the checkout available."""
    build = tmp_path / "build"
    message = "campaign failed"
    with pytest.raises(RuntimeError, match=message), mutation_lease.lease(build):
        raise RuntimeError(message)
    with mutation_lease.lease(build):
        pass


def test_the_mutation_task_holds_the_lease_for_the_whole_campaign(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Workspace removal, coverage, and evidence capture all happen under it."""
    events: list[str] = []

    class Lease:
        """Record entry and exit."""

        def __enter__(self) -> None:
            events.append("acquire")

        def __exit__(self, *_arguments: object) -> None:
            events.append("release")

    monkeypatch.setattr(sys, "platform", "linux")
    monkeypatch.setattr(
        tasks.mutation_lease, "lease", lambda build: Lease() if build else None
    )
    monkeypatch.setattr(
        tasks.hypothesis_runner,
        "run_isolated",
        lambda *_arguments: events.append("campaign"),
    )
    tasks._mutation()  # ruff: ignore[private-member-access] - task contract.
    assert events == ["acquire", "campaign", "release"]


def test_a_platform_without_file_locking_refuses_before_touching_the_checkout(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Mutation testing is POSIX-only; the lease says so instead of failing oddly."""
    monkeypatch.setattr(mutation_lease, "fcntl", None)
    build = tmp_path / "build"
    with pytest.raises(RuntimeError) as raised, mutation_lease.lease(build):
        pass
    assert str(raised.value) == "mutation testing requires POSIX file locking"
    assert not build.exists()

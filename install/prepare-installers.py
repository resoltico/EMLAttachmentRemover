#!/usr/bin/env python3
"""Stage release installers with help links bound to their source revision."""
import argparse
from pathlib import Path
import re


MARKERS = {
    "install.sh": b"docs_revision='main'",
    "install.ps1": b"$documentationRevision = 'main'",
}


def prepare(source: Path, output: Path, revision: str) -> None:
    if not re.fullmatch(r"main|[0-9a-f]{40}|[0-9a-f]{64}", revision):
        raise ValueError("revision must be main or a lowercase full Git commit hash")
    source = source.resolve()
    output = output.resolve()
    if output == source:
        raise ValueError("output must be a separate installer staging directory")
    staged = {}
    for name, marker in MARKERS.items():
        contents = (source / name).read_bytes()
        if contents.count(marker) != 1:
            raise ValueError(f"{name} must contain exactly one documentation revision marker")
        destination = output / name
        if destination.is_symlink() or (destination.exists() and destination.samefile(source / name)):
            raise ValueError(f"refusing to overwrite source through {destination}")
        staged[name] = contents.replace(marker, marker.replace(b"main", revision.encode("ascii")), 1)
    output.mkdir(parents=True, exist_ok=True)
    for name, contents in staged.items():
        (output / name).write_bytes(contents)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    try:
        prepare(Path(__file__).resolve().parent, args.output_dir, args.revision)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Cannot stage installers: {error}\n")


if __name__ == "__main__":
    main()

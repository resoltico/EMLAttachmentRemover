#!/usr/bin/env python3
"""Stage the standalone installers and hashes beside locally built CLI archives."""
import argparse
import hashlib
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    archives = sorted(p for p in directory.iterdir() if p.name.endswith((".tar.gz", ".zip")))
    if not archives:
        parser.error("directory contains no built archives")
    source = Path(__file__).resolve().parent
    for name in ("install.sh", "install.ps1"):
        with (directory / name).open("xb") as stream:
            stream.write((source / name).read_bytes())
    files = archives + [directory / "install.sh", directory / "install.ps1"]
    with (directory / "SHA256SUMS").open("x", encoding="ascii", newline="\n") as stream:
        for path in sorted(files):
            stream.write(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")
    print(f"Prepared installer test release: {len(archives)} archive(s), two installers")


if __name__ == "__main__":
    main()

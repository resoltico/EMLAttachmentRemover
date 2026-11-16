#!/usr/bin/env python3
"""Stage the standalone installers and hashes beside locally built CLI archives."""
import argparse
import hashlib
import subprocess
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--revision", default="main", help="full source commit for published installer Help")
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    archives = sorted(p for p in directory.iterdir() if p.name.endswith((".tar.gz", ".zip")))
    if not archives:
        parser.error("directory contains no built archives")
    source = Path(__file__).resolve().parent
    for name in ("install.sh", "install.ps1", "SHA256SUMS"):
        if (directory / name).exists() or (directory / name).is_symlink():
            parser.error(f"staging refuses to overwrite {name}")
    subprocess.run([
        sys.executable, str(source / "prepare-installers.py"),
        "--revision", args.revision, "--output-dir", str(directory),
    ], check=True)
    files = archives + [directory / "install.sh", directory / "install.ps1"]
    with (directory / "SHA256SUMS").open("x", encoding="ascii", newline="\n") as stream:
        for path in sorted(files):
            stream.write(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")
    print(f"Prepared installer test release: {len(archives)} archive(s), two installers")


if __name__ == "__main__":
    main()

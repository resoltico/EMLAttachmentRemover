#!/usr/bin/env python3
"""Compare the vendored installers with their pinned PDFConcat engine."""
import argparse
import difflib
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

FILES = ("install.sh", "install.ps1")
SHELL_KEYS = {
    "product_name", "command_name", "repo_url", "env_prefix", "archive_prefix",
    "archive_separator", "archive_versioned", "archive_rooted", "macos_token", "x64_token",
    "arm64_token", "checksum_name", "minimum_macos", "legal_files",
}
PS_KEYS = {
    "name", "command", "repository", "environment", "archiveprefix", "separator",
    "versionedarchive", "rootedarchive", "macos", "x64", "arm64", "checksums", "windowsfolder",
    "minimummacos", "legalfiles",
}
BEGIN = "# BEGIN PRODUCT SETTINGS"
END = "# END PRODUCT SETTINGS"
SHELL_LITERAL = re.compile(r"[a-z_][a-z0-9_]*=(?:'[^']*'|[A-Za-z0-9_./:-]+)")
PS_STRING = r"'(?:[^']|'')*'"
PS_VALUE = rf"(?:{PS_STRING}|[0-9]+|\$(?:true|false)|@\(\s*(?:{PS_STRING}(?:\s*,\s*{PS_STRING})*)?\s*\))"
PS_LITERAL = re.compile(rf"\$?[A-Za-z_][A-Za-z0-9_]*\s*=\s*{PS_VALUE}", re.IGNORECASE)
PS_TABLE = re.compile(r"\$[A-Za-z_][A-Za-z0-9_]*\s*=\s*(?:\[ordered\])?@\{", re.IGNORECASE)


def engine(data, name):
    """Remove only literal product settings, preserving all executable logic."""
    text = data.decode("utf-8").replace("\r\n", "\n")
    if "\r" in text:
        raise ValueError(f"{name}: bare carriage return")
    lines = text.splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if line.strip() == BEGIN]
    ends = [i for i, line in enumerate(lines) if line.strip() == END]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        raise ValueError(f"{name}: expected exactly one bounded product settings block")
    first, last = starts[0], ends[0]
    table = False
    seen = set()
    for line in lines[first + 1:last]:
        value = line.strip()
        if not value or value.startswith("#"):
            continue
        if name.endswith(".sh"):
            valid = SHELL_LITERAL.fullmatch(value)
        elif PS_TABLE.fullmatch(value) and not table:
            table = True
            continue
        elif value == "}" and table:
            table = False
            continue
        else:
            valid = PS_LITERAL.fullmatch(value)
        key = value.split("=", 1)[0].strip().lstrip("$").lower()
        allowed = SHELL_KEYS if name.endswith(".sh") else PS_KEYS
        if key not in allowed or key in seen:
            raise ValueError(f"{name}: unknown or duplicate product setting: {key}")
        seen.add(key)
        if not valid:
            raise ValueError(f"{name}: product settings must contain literal data only: {value}")
    if table:
        raise ValueError(f"{name}: unclosed product settings table")
    return "".join(lines[:first + 1] + lines[last:])


class HttpsRedirects(urllib.request.HTTPRedirectHandler):
    """Keep development-time pinned-source downloads on HTTPS too."""
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != "https":
            raise ValueError("Pinned installer source redirected outside HTTPS")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def reference(name, revision, checkout):
    if checkout:
        return subprocess.check_output(
            ["git", "-C", str(checkout), "show", f"{revision}:install/{name}"], timeout=30
        )
    url = f"https://raw.githubusercontent.com/resoltico/pdfconcat/{revision}/install/{name}"
    with urllib.request.build_opener(HttpsRedirects()).open(url, timeout=30) as response:
        data = response.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("Pinned installer source exceeds one MiB")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkout", nargs="?", type=Path, help="local PDFConcat checkout for offline verification")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    revision = (root / "engine-source.txt").read_text(encoding="ascii").strip()
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", revision):
        raise ValueError("engine-source.txt must contain a full lowercase Git commit ID")
    failed = False
    for name in FILES:
        expected = engine(reference(name, revision, args.checkout), name)
        actual = engine((root / name).read_bytes(), name)
        if actual != expected:
            failed = True
            sys.stderr.writelines(difflib.unified_diff(
                expected.splitlines(True), actual.splitlines(True),
                fromfile=f"pdfconcat/{revision}/{name}", tofile=f"{root.parent.name}/{name}",
            ))
    if failed:
        raise ValueError("Installer engines differ; synchronize the reviewed PDFConcat engine and pin")
    print(f"Installer engines match PDFConcat {revision}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(str(error))

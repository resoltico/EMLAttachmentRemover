"""Define filename and text-content rules for public repository files."""

from __future__ import annotations

import re
import socket
from pathlib import Path
from typing import Final

MAIL_SUFFIXES: Final = frozenset({".eml", ".mbox", ".msg", ".ost", ".pst"})
PRIVATE_SUFFIXES: Final = frozenset({
    ".jks",
    ".kdbx",
    ".key",
    ".keystore",
    ".p12",
    ".p8",
    ".pem",
    ".pfx",
    ".pkcs12",
    ".pk8",
    ".pkcs8",
    ".ppk",
})
CREDENTIAL_STEMS: Final = frozenset({
    "auth",
    "client_secret",
    "client_secrets",
    "credential",
    "credentials",
    "secret",
    "secrets",
    "service-account",
    "service_account",
    "token",
    "tokens",
})
PRIVATE_EXACT_NAMES: Final = frozenset({
    ".env",
    ".envrc",
    ".netrc",
    ".npmrc",
    ".pypirc",
    "_netrc",
})
PRIVATE_KEY_NAMES: Final = ("id_dsa", "id_ecdsa", "id_ed25519", "id_rsa")
EXCLUDED_DIRECTORY_NAMES: Final = frozenset({"__pycache__"})
EXCLUDED_FILE_NAMES: Final = frozenset({".DS_Store", "Desktop.ini", "Thumbs.db"})
TOOL_CACHE_DIRECTORY_NAMES: Final = frozenset({
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
})
PYTHON_BYTECODE_SUFFIXES: Final = (".pyc", ".pyo")
GENERIC_HOSTNAMES: Final = frozenset({
    "localhost",
    "localhost.localdomain",
    "machine_host",
})


def generated_file(name: str) -> bool:
    """Return whether a regular nested file is recognized generated output.

    Returns:
        Whether the path can be omitted from the public manifest.

    """
    return name in EXCLUDED_FILE_NAMES or name.endswith(PYTHON_BYTECODE_SUFFIXES)


def cache_directory_reason(name: str) -> str | None:
    """Return an actionable reason for a repository-local cache directory.

    Returns:
        A stable removal diagnostic, or ``None`` for a non-cache directory.

    """
    if name == "__pycache__":
        return (
            "Python bytecode cache directory is prohibited because bytecode can "
            "embed machine-specific paths; run Python with bytecode disabled and "
            "remove this path"
        )
    if name in TOOL_CACHE_DIRECTORY_NAMES:
        return (
            "repository-local tool cache directory is prohibited because cache "
            "data is not public evidence; disable caching or use ephemeral storage "
            "and remove this path"
        )
    return None


def cache_file_reason(name: str) -> str | None:
    """Return an actionable reason for a repository-local cache file.

    Returns:
        A stable removal diagnostic, or ``None`` for a non-cache file.

    """
    if name.endswith(PYTHON_BYTECODE_SUFFIXES):
        return (
            "Python bytecode cache is prohibited because it can embed "
            "machine-specific paths; run Python with bytecode disabled and remove "
            "this path"
        )
    if name == ".coverage" or name.startswith(".coverage."):
        return (
            "repository-local coverage data is prohibited because it can embed "
            "machine-specific paths; use ephemeral coverage storage and remove "
            "this path"
        )
    return None


def generated_root_file(name: str, *, mutmut_statistic: bool) -> bool:
    """Return whether a regular root file is recognized generated output.

    Returns:
        Whether the root file can be omitted from the public manifest.

    """
    return generated_file(name) or (
        name == ".coverage" or name.startswith(".coverage.") or mutmut_statistic
    )


def generated_directory(name: str) -> bool:
    """Return whether a nested directory is recognized generated output.

    Returns:
        Whether the directory can be omitted from public traversal.

    """
    return name in EXCLUDED_DIRECTORY_NAMES or name.endswith(".egg-info")


def private_file_reason(name: str) -> str | None:
    """Return the policy reason for a sensitive filename, when one applies.

    Returns:
        An actionable reason, or ``None`` for a permitted filename.

    """
    folded = name.casefold()
    suffixes = Path(folded).suffixes
    mail_suffix = next((item for item in suffixes if item in MAIL_SUFFIXES), None)
    private_suffix = next((item for item in suffixes if item in PRIVATE_SUFFIXES), None)
    credential_stem = folded
    while Path(credential_stem).suffix:
        credential_stem = Path(credential_stem).stem
    if mail_suffix is not None:
        reason = f"private mail format {mail_suffix!r} is prohibited"
    elif (
        folded in PRIVATE_EXACT_NAMES
        or any(
            folded.startswith(f"{private_name}{separator}")
            for private_name in PRIVATE_EXACT_NAMES
            for separator in (".", "-", "_")
        )
        or ".env" in suffixes
    ):
        reason = "environment or credential filename is prohibited"
    elif private_suffix is not None:
        reason = f"private-key or credential extension {private_suffix!r} is prohibited"
    elif credential_stem in CREDENTIAL_STEMS or any(
        credential_stem.startswith(f"{stem}{separator}")
        for stem in CREDENTIAL_STEMS
        for separator in ("-", "_")
    ):
        reason = "credential-bearing filename is prohibited"
    elif not folded.endswith(".pub") and any(
        folded == key_name
        or any(
            folded.startswith(f"{key_name}{separator}") for separator in (".", "-", "_")
        )
        for key_name in PRIVATE_KEY_NAMES
    ):
        reason = "private-key filename is prohibited"
    else:
        reason = None
    return reason


def public_content_messages(text: str) -> tuple[str, ...]:
    """Return line-specific diagnostics for non-public text content.

    Returns:
        Every private-identity and concrete-home-path diagnostic in line order.

    """
    messages: list[str] = []
    hostname = socket.gethostname().strip()
    hostname_pattern = (
        None
        if not hostname or hostname.casefold() in GENERIC_HOSTNAMES
        else re.compile(
            rf"(?i)(?<![\w.-]){re.escape(hostname)}(?![\w.-])",
        )
    )
    for line_number, line in enumerate(text.splitlines(), start=1):
        addresses = re.finditer(
            r"(?i)(?<![\w.+-])[\w.+-]+@"
            r"(?P<domain>(?:[a-z0-9-]+\.)+[a-z]{2,63})(?![\w.-])",
            line,
        )
        if any(
            match.group("domain").casefold() != "example.test" for match in addresses
        ):
            messages.append(
                f"non-reserved email identity on line {line_number}; replace it "
                "with an example.test identity",
            )
        home_patterns = (
            r"/" + "Users" + r"/(?![<$%{])[^/\s\"']+",
            r"/" + "home" + r"/(?![<$%{])[^/\s\"']+",
            r"(?i)[a-z]:\\" + "Users" + r"\\(?![<$%{])[^\\\s\"']+",
        )
        if any(re.search(pattern, line) for pattern in home_patterns):
            messages.append(
                f"user-home path on line {line_number}; replace it with a portable "
                "placeholder",
            )
        if hostname_pattern is not None and hostname_pattern.search(line):
            messages.append(
                f"local hostname on line {line_number}; replace it with MACHINE_HOST",
            )
    return tuple(messages)


def generated_content_messages(
    path: Path,
    *,
    allow_opaque: bool = False,
) -> tuple[str, ...]:
    """Return private-content issues for one generated text artifact.

    Returns:
        Private-data, opacity, or read-error diagnostics.

    """
    try:
        text = path.read_bytes().decode()
    except UnicodeDecodeError:
        if allow_opaque:
            return ()
        return (
            (
                "opaque generated file cannot be audited for private machine data; "
                "remove it or publish it only as an explicitly validated "
                "build/release artifact"
            ),
        )
    except OSError as error:
        return (f"cannot read generated file: {error}",)
    return public_content_messages(text)

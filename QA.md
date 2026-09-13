# Quality assurance contract

The v3 contract is MIME structure and source-byte fidelity, not visual equivalence.
Public tests use only synthetic messages; field EML files, paths, hashes, screenshots,
and observations stay outside this repository and public CI artifacts.

## Required local gates

```sh
uv lock --check
uv sync --locked --group dev --python 3.14.7
uv run ruff format --check src tests tools
uv run ruff check src tests tools
uv run mypy --no-incremental --cache-dir /tmp/eml-remover-mypy src
uv run pytest -p no:cacheprovider
uv run coverage run -m pytest -p no:cacheprovider
uv run coverage report --show-missing
uv run python tools/build_zipapp.py
```

The repository gate requires 100% statement and branch coverage. Mutation work is not
complete until every actionable mutation has been killed or removed as a genuinely
equivalent surface, and the campaign is rerun fresh after code freeze. Coverage,
mutation, static checks, build reproducibility, archive inspection, isolated install,
workflow checks, secrets/hygiene checks, and platform jobs are release gates—not
thresholds to weaken.

The canonical end-to-end commands are `uv run python tools/tasks.py quality`,
`thorough`, `mutation`, `build`, and `release`. `quality` uses the deterministic
Hypothesis profile; `thorough` is the larger randomized exploration pass. Run both
standard and free-threaded CPython quality lanes locally when the platform supports
them; Mutmut requires a fork-capable POSIX runtime.

“JUnit XML” here names the conventional xUnit2 interchange format emitted by the
pinned `pytest==9.1.1` test runner and sanitized by `tools/junit_report.py`. The
project has no Java/JVM test runtime, so a Java JUnit dependency—including JUnit
6.1.3—would be unrelated and must not be added to `pyproject.toml`; GitHub Actions
uses the lockfile-pinned pytest version to create the report.

## MIME and raw-wire evidence

Tests must cover the closed policy matrix: disposition, filename/name, CID/location,
plain and HTML alternatives, `multipart/mixed`, `multipart/related`, nested supported
containers, protected/signature types, unknown multipart/message types, and all
ambiguous roles. A successful removal has exactly one reason:
`EXPLICIT_ATTACHMENT` or `RELATED_NONROOT_COMPONENT`.

The raw executor may only delete source-indexed subtree or complete header-field
spans. Tests cover CRLF/LF/CR transport, `7bit`, `8bit`, `binary`, base64 and
quoted-printable payloads, malformed retained CTE rejection, and malformed discarded
payload acceptance. Candidate verification reparses from bytes, recomputes retained
payload fingerprints, checks retained structure/order and digest, and requires a
second policy pass to request no work.

## Filesystem and state evidence

Tests cover unnormalized symlink traversal, regular-file source binding, source and
destination aliases, exact existing verification, output conflicts, dry-run parity,
no-replace races, mode `0600`, staged-temp cleanup, cancellation, and ledger
completeness. A multi-input report has one terminal record per argv request in order;
partial work is never erased by a later malformed input.

Before a public release, independently replay every documented v2.0.1 issue against
v3, inspect the schema/Finder consumer and no-replace boundary, run packaged artifacts
rather than imports alone, and complete the private field protocol locally. The
release workflow must gate publication on terminal-green quality, mutation, archive,
checksum, provenance, and artifact-identity jobs.

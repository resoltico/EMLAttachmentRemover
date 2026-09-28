# Quality assurance contract

The product contract is MIME structure and source-byte fidelity, not visual equivalence.
Public tests use only synthetic messages; field EML files, paths, hashes, screenshots,
and observations stay outside this repository and public CI artifacts.

## Required local gates

Run every CI gate this host can reproduce before pushing:

```sh
uv run python tools/tasks.py ci
```

`ci` installs both CI interpreters (CPython 3.14.7 and free-threaded 3.14.7t) into
private temporary environments from the lockfile, then runs, cheapest first: both
`quality` lanes, type checks for every CI runner OS, the tag check, the release
build and its verification, a fresh `mutation` campaign, and both randomized
`thorough` lanes with observation finalization. Mutation runs natively on Linux and,
on macOS, through Docker in CI's runner image (Ubuntu 24.04, the pinned uv, and an
unprivileged user), leaving its evidence in `build/linux-mutation/`; Windows skips
it, as CI does. Pass `--release-tag vX.Y.Z` before tagging. Windows lanes, Linux-only behavior, artifact
attestation, and GitHub publication remain CI-only. A test fails when a workflow
`run:` step has no local counterpart, so the two cannot drift silently.

The component tasks are `uv run python tools/tasks.py check`, `coverage`, `quality`,
`test`, `thorough`, `mutation`, `build`, and `release`. `quality` uses the
deterministic Hypothesis profile; `thorough` is the larger randomized exploration
pass. Tests require the private storage that `tools/tasks.py` provides; running
`pytest` directly needs an absolute `HYPOTHESIS_STORAGE_DIRECTORY` outside the
checkout and is a diagnostic aid, not a gate.

The repository gate requires 100% statement and branch coverage. Mutation work is not
complete until every actionable mutation has been killed or removed as a genuinely
equivalent surface, and the campaign is rerun fresh against the release source.
Coverage, mutation, static checks, build reproducibility, archive inspection, isolated
install, workflow checks, secrets/hygiene checks, and platform jobs are release
gates—not thresholds to weaken. Coverage XML is published even when tests or the
threshold fail; the failure still fails the gate.

## Task lifecycle and mutation evidence

Every task command runs in its own POSIX process group. A timeout or Ctrl-C sends the
group SIGINT, then SIGKILL after a grace period; after a normal exit, leftover group
members are killed. No descendant can write evidence once its task step returns.
Mutmut requires a fork-capable POSIX runtime.

Mutant IDs depend on which lines each OS's tests cover, and the reviewed equivalents
are Linux IDs, so only a Linux campaign is qualifying evidence; a native macOS
`mutation` run is exploratory.

`mutation` first checks that `tools/equivalent_mutants.json` is bound to the current
`src/` and `tools/` source, before removing any workspace or running any test. Any
change to a Python file there invalidates the binding. To rebind after review:

```sh
uv run python tools/tasks.py mutation --allow-stale-manifest
uv run python tools/check_mutation_results.py --source-sha256
uv run python tools/check_mutation_results.py
```

The first command skips only the preflight; its final result gate still fails on a
stale manifest. Review every surviving mutant, update the manifest entries and its
`source_sha256` to the printed digest, then rerun the final check against the same
evidence. `--workers auto|1-64` selects Mutmut parallelism; worker count never
changes a verdict.

“JUnit XML” names the conventional xUnit2 interchange format emitted by the
lockfile-pinned pytest runner and sanitized by `tools/junit_report.py`. The project
has no Java/JVM runtime; the report is an interchange artifact, not a Java dependency.

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

Machine JSON is canonical ASCII (`ensure_ascii=true`, sorted keys, no non-finite
numbers) on a binary channel. Native POSIX path evidence remains Base64-authoritative
when a path has no portable Unicode text; tests cover direct, streamed, recovery, and
Finder-consumer behavior under hostile display codecs.

The parser supports one root Unix-From envelope for a single message, preserving its
bytes and original coordinates. It is not mbox processing and does not reinterpret
interior `From ` body data.

Before a public release, inspect the schema/Finder consumer and no-replace boundary,
run packaged artifacts rather than imports alone, and complete the private field
protocol locally. The release workflow must gate publication on terminal-green
quality, mutation, archive, checksum, provenance, and artifact-identity jobs.

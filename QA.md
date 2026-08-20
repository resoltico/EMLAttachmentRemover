# Quality assurance contract

This document defines the repeatable acceptance gates for EML Attachment Remover.
It intentionally avoids test, statement, branch, and mutant totals: those are
transient evidence emitted by the tools, while the thresholds below are the durable
contract.

## Project and data policy

- Project metadata, including the application version, has one source of truth:
  `pyproject.toml`.
- The supported runtime is CPython 3.14.x. `.python-version` and CI select CPython
  3.14.7.
- Hatchling 1.32.0 is pinned as both the PEP 517 build backend and a locked build/QA
  tool. Runtime dependencies are empty; development and QA dependencies are pinned
  in `pyproject.toml` and `uv.lock`.
- The project is MIT-licensed and attributed to Ervins Strauhmanis, using the same
  ASCII spelling in project metadata and `LICENSE`.
- Tests and examples contain only public, synthetic data. Test messages use reserved
  `example.test` identities and generated payloads. Private field-test messages stay
  outside the repository and every generated report or uploaded artifact; generated
  Hypothesis evidence is sanitized before upload.

## Text-only transformation contract

The sole processing scope is `text-only`. A successful plan selects a safe
resource-free `text/plain` body, discards every unselected alternative body as one
atomic subtree with its complete dependent-resource closure, and removes ordinary
attachments elsewhere. Reported MIME paths are bound to the pre-transformation source
tree. The selected decoded text remains identical after canonical MIME newline
normalization (`CRLF`, `CR`, and `LF` become `LF`) and is bound to the source by
SHA-256. A sole unselected HTML representation may supply only local layout when its
visible projection has exactly the same non-whitespace text; that projection is bound
as the expected output payload. If the selected body is nested, execution promotes the
verified text to a non-multipart root `text/plain` entity. Plain-only promotion retains
the selected content-transfer encoding, encoded payload, and applicable `Content-*`
headers; projected text uses canonical UTF-8 plain text. Ordered root message/envelope
headers remain, while invalidated size/attachment markers and wrapper preamble and
epilogue text are removed. A safe root plain message with no action is copied
byte-for-byte.

After the immutable source-path action plan executes, the temporary output is reparsed
and must match the retained root's leaf fingerprint and structure. A second text-only
planning pass must report `modified == false`, proving that no discard or body-promotion
action remains and that every modified result is wrapper-free root `text/plain`. The
original remains byte-identical, and the verified derived output is published
atomically.
Discarded subtrees are verified by source-path removal and that second-pass invariant,
not by requiring their payload hashes to be globally absent: selected and discarded
parts may legitimately contain identical bytes.

The planner fails closed when no safe text-only representation can be proven. It never
uses arbitrary HTML as a content source: a local formatting projection is permitted
only after exact non-whitespace equality with the selected plain body. It does not
fetch content, perform OCR, classify images, or use sender, filename, size, or
media-content heuristics. Signed, encrypted, and opaque security content needed by the
selected body, or with an ambiguous structural role, cannot satisfy the invariant and
produces no output. An explicit attachment or unselected protected subtree can be
discarded atomically without semantic interpretation or partial rewriting, after
global MIME and transfer-encoding validation.

Machine-readable command reports use exact schema version `2` and scope
`text-only`. Successful result objects separately report selected plain-text bodies,
discarded body representations, discarded body resources, and removed ordinary
attachments. Each discarded resource exposes the ordered, possibly empty list of
source body MIME paths that referenced it. The selected-body path likewise remains a
source audit path even though a modified derived message promotes that body to its
root. Finder validates the schema and scope before displaying or revealing any path.
An existing output accepted through `--skip-existing` is explicitly unverified and
must not be described as a newly verified output.

## Canonical local qualification

Run the locked standard-interpreter gates from the repository root:

```sh
uv sync --locked --group dev --python 3.14.7
uv run python tools/tasks.py quality
uv run python tools/tasks.py thorough
uv run python tools/tasks.py mutation
uv run python tools/tasks.py build
uv run python tools/tasks.py release
```

`quality` runs all static and repository checks, then the complete pytest suite under
Coverage.py with the deterministic `project-ci` Hypothesis profile. `mutation`
independently establishes fresh coverage and a fresh Mutmut workspace before its
full campaign. The separate `thorough` command runs the larger randomized Hypothesis
campaign. Use `thorough --observable --timeout-seconds 3300` to mirror the observable
weekly campaign locally.

Repeat the runtime-sensitive gates under free-threaded CPython:

```sh
uv sync --locked --group dev --python 3.14.7t
uv run python tools/tasks.py quality
uv run python tools/tasks.py thorough
```

For a faster development loop, `tools/tasks.py test` selects the development
Hypothesis profile. The task runner is the portable interface on Windows, macOS, and
Linux; no Make implementation is required. Mutation testing is the one native
Windows exception because Mutmut requires process-fork support; use WSL, Linux, or
macOS for that task.

## Generated evidence and the “JUnit XML” name

Successful test and coverage runs write ignored machine-readable evidence beneath
`build/`, including profile-named pytest xUnit 2 XML and `coverage.xml`. Mutation
testing exports aggregate statistics under `mutants/` and sorted named results under
`build/`. These files are local evidence, not source or release artifacts.

Pytest 9.1.1 is the sole test runner and test-result XML producer. It is pinned in
`pyproject.toml` and `uv.lock`, and each workflow installs that lock with
`uv sync --locked --group dev`. Pytest creates the report in the external private
test root; the project-owned Python helper `tools/junit_report.py` replaces known
machine-path prefixes and the local hostname, rejects repository privacy-policy
violations, validates pytest's bounded xUnit 2 hierarchy, and atomically publishes
valid XML. Hypothesis statistics properties additionally require canonical Base64,
strict UTF-8, exact node-ID binding, and the same checks after decoding. That sequence
also prevents a report being inspected halfway through its own test run. “JUnit XML”
is only the conventional name used by report consumers for this interchange format:
the repository contains no Java/JVM test sources, build system, JUnit runtime, or
JUnit dependency. GitHub Actions uploads the already-produced XML and neither
resolves nor executes JUnit.

## Pytest contract

Pytest is configured in `pyproject.toml` with strict configuration and marker checks
and warnings promoted to errors. The task runner additionally uses Python development
mode and `-W error`, prints verbose test identities and Hypothesis statistics, and
applies subprocess timeouts.

The suite combines focused unit tests, command-line and subprocess tests,
distribution and shell integration tests, exact MIME-oracle properties, raw-wire
properties, and a stateful output-file model. Platform-specific tests are exercised
on the platforms that provide the relevant capability; the CI matrix supplies
Windows and POSIX environments instead of pretending one host can validate both.
There are no optional private fixtures or local-email prerequisites.

## Hypothesis wiring and findings

`tests/__init__.py` loads `tests/hypothesis_config.py` before test modules. The
configuration registers four complete profiles and rejects every unknown profile:

| Profile | Examples per property | Derandomized | Database | Purpose |
| --- | ---: | --- | --- | --- |
| `project-development` | 250 | no | private ephemeral database | normal local tests |
| `project-ci` | 500 | yes | disabled | quality and coverage |
| `project-thorough` | 2,000 | no | private ephemeral database | exploration |
| `project-mutation` | 50 | yes | disabled | bounded mutant evaluation |

Configuration requires `HYPOTHESIS_STORAGE_DIRECTORY` to be an absolute path
outside the project. The canonical task runner creates and owns that private path;
a direct pytest invocation without this boundary fails before collection instead of
creating an opaque repository cache.

Every profile explicitly enables all phases, multiple-bug reporting, reproduction
blobs, normal verbosity, no suppressed health checks, and 50 state-machine steps.
All use `deadline=None`: per-example timing is unsuitable for the intentional
filesystem and subprocess work. Bounded task and job timeouts prevent hangs instead:

- local `test` and `thorough` subprocesses: 10 and 30 minutes;
- local mutmut subprocess: two hours;
- GitHub quality and release qualification/build jobs: 30 minutes;
- GitHub randomized exploration: 60 minutes;
- GitHub mutation testing: 180 minutes;
- GitHub release publishing: 15 minutes.

Generated properties do useful oracle work rather than merely executing lines. They
compare the complete immutable transformation plan with the serialized result,
preserve selected decoded text exactly after canonical newline normalization, require
the serialized result to satisfy the second-pass text-only invariant, isolate
attached-message references, generate adversarial CID and Content-Location dependency
closures, exercise valid and malformed transfer encodings, and model
create/conflict/replace/dry-run/source-alias state transitions.
Events and targets describe the shapes, encodings, sizes, depths, selections, and
transitions explored.

Configuration regression tests inspect every effective profile with ambient CI both
set and absent. Another subprocess test deliberately fails a property, verifies
minimization and the emitted `@reproduce_failure` blob, and replays that blob. A real
counterexample therefore has an unambiguous path:

1. pytest exits non-zero locally or the GitHub check turns red;
2. the console log, Hypothesis statistics, and pytest XML report identify the property;
3. Hypothesis prints minimized inputs and a reproduction blob;
4. reproduce and diagnose the defect;
5. fix the implementation and retain a readable public regression or `@example`;
6. rerun `quality` and `thorough`.

The development and thorough profiles retain replay examples only inside the
mode-private external storage owned by one canonical task run. The weekly exploration
workflow enables Hypothesis observability and uploads only sanitized
`.hypothesis/observed` artifacts even when exploration fails; it neither restores,
saves, nor uploads an opaque replay database. Durable findings instead use the emitted
reproduction blob and become reviewed public regression tests or explicit examples.
Tests must never derive generated values from personal, production, credential, or
environment data. Before publication, the
finalizer nulls per-example coverage, removes process metadata, redacts unbounded
raw argument values and the duplicate rendered call while retaining argument names,
replaces known private prefixes, and rejects any remaining absolute POSIX, Windows,
UNC, or `file:` path in every retained field. A complete sanitized set is staged
before it replaces the prior publication; promotion failures restore the prior set,
and a failed restore retains its only backup for recovery.
Canonical local and CI test, coverage, thorough, and mutation processes override
ambient Hypothesis storage with unique mode-private OS-temporary directories outside
the repository. The explicit example database, constants, patch suggestions, and raw
observations all remain inside that owned storage for the whole session. Every exit
path, including interrupts and publication failures, attempts validated removal;
cleanup failure is surfaced, and concurrent failures are grouped rather than hidden.
Of the Hypothesis observation data, observable runs publish only sanitized JSONL.
Accepted findings must become reviewed tests or explicit examples.

## Coverage, static analysis, and design

Coverage.py records both lines and branches for exactly these repository-owned
execution scopes:

```text
src/eml_attachment_remover
tools
```

`fail_under = 100` makes any missed statement or branch fail. Subprocess coverage is
enabled so black-box tool and CLI execution contributes correctly. Tests are not
inflated into the production coverage denominator, but they are still checked by
Ruff, strict Mypy, and the uniform design budget. Raw Coverage databases and shards
exist only inside the validated OS-temporary test root. Coverage XML is rendered
there, bound to the exact configured source roots and pinned Coverage.py producer,
rejected on malformed structure, inconsistent totals, private content, or absolute
paths, and only then atomically published under `build/`.

The `check` task runs repository hygiene before any recursive analyzer, then:

- Ruff formatter verification and all-rule preview linting with narrow documented
  compatibility and test-framework exceptions;
- Mypy 2.3.1 in strict mode across `src`, `tests`, and `tools`, with its extra,
  unreachable-code, strict-bytes, and strict-`None`-equality checks enabled;
- a no-exceptions module-design gate across those same trees: at most 450 physical
  lines, 400 substantive (nonblank, non-comment-only) lines, 20 top-level
  declarations, and 20 direct methods per class, with no grandfathered files;
- Ruff function budgets for complexity, statements, branches, locals, returns,
  arguments, boolean expressions, and nesting;
- `actionlint` for every workflow, with ShellCheck and Pyflakes integration;
- direct ShellCheck for every macOS integration shell script;
- repository hygiene and secret scanning.

The canonical commands disable Python bytecode, pytest and Ruff caches, and Mypy
incremental caching. Repository-local `__pycache__`, bytecode, `.pytest_cache`,
`.ruff_cache`, `.mypy_cache`, and `.coverage*` artifacts fail hygiene because opaque
cache data can embed machine-specific paths and is not public evidence.

## Mutation testing

Mutmut changes only covered lines under `src/eml_attachment_remover` and `tools`, and
runs pytest with the deterministic, database-free `project-mutation` profile. The
canonical task first establishes coverage and deletes only the exact generated
`mutants/` workspace, preventing remembered results from passing as fresh evidence.
It then captures every named status in lexical order and exports aggregate JSON even
when an earlier evidence step fails.

The independent parser requires aggregate and named evidence to agree exactly and a
100% actionable score. Surviving equivalents require an exact mutant ID and nonempty
public rationale in `tools/equivalent_mutants.json`. The manifest is bound to a
deterministic SHA-256 digest of all production Python paths and bytes, so a source
change invalidates every prior review. Stale/missing manifest IDs and untested,
skipped, suspicious, timed-out, interrupted, segfaulted, unknown, or unexported
statuses fail.

The mutation workflow runs weekly and on manual dispatch. It uploads aggregate and
named mutation evidence, the source-bound reviewed-equivalent manifest, coverage XML,
and the CI-profile pytest XML report even on failure.

## Public-repository hygiene and secrets

`tools/check_repository_hygiene.py` walks the workspace directly, including paths
ignored by Git. Root-level Git internals and the third-party virtual environment
remain opaque; expected generated build and report roots are recursively scanned for
private names and UTF-8 content. Only direct files with reviewed build/release archive
names may remain opaque here; their contents are validated by the separate build and
release verifiers. Links and non-regular objects in generated roots also fail. The
audit rejects:

- private mail containers: `.eml`, `.msg`, `.mbox`, `.pst`, and `.ost`;
- real environment, credential, secret, token, keystore, and private-key files;
- undocumented top-level files or directories;
- repository-local bytecode, tool caches, and raw Coverage databases;
- every symbolic link in the public source surface;
- FIFOs, sockets, devices, and other non-regular public artifacts.

The allowed root surface is explicit; this runtime-dependency-free project ships no
environment file, including placeholder variants. Every public source-manifest file
must be UTF-8 text, use reserved `example.test` identities, omit concrete macOS,
Linux, and Windows user-home paths, and have a name that is safe and non-colliding
across supported filesystems. Direct reviewed build/release archive shapes are the
only opaque exception. The audit produces a deterministic source manifest, which
`detect-secrets --no-verify` scans without a baseline or path suppression.
Diagnostics name every offending path and the required remediation; the gate does
not delete files.

## GitHub automation

The workflows use minimum permissions, immutable commit pins for third-party
actions, non-persistent checkout credentials, concurrency controls, and explicit job
timeouts.

- `quality.yml` runs on pull requests, pushes to `main`, and manual dispatch. Its
  matrix covers Ubuntu, macOS, and Windows on standard and free-threaded CPython
  3.14.7, and uploads pytest xUnit 2 and coverage XML.
- `hypothesis.yml` runs weekly and manually. It executes the observable, randomized
  thorough profile across the same platform/interpreter matrix with mode-private
  external storage and publishes only sanitized observation artifacts; opaque replay
  data is neither restored, retained, nor uploaded.
- `mutation.yml` runs weekly and manually on canonical Linux CPython, establishes
  fresh coverage and Mutmut state, requires a 100% actionable score, and preserves
  reviewable evidence.
- `release.yml` runs only for tags. It first requires the full quality matrix and an
  exact match between the tag and `pyproject.toml` version, then requires a fresh
  same-tag mutation result, builds, redownloads and reverifies the exact artifacts,
  attests, and publishes from canonical Linux CPython. Publication also requires the
  exact tag-named `.github/release-notes/vVERSION.md` file and binds both the release
  title and body to that validated version.

Remote status, scheduled execution, release publication, and provenance attestations
exist only after this source is hosted on GitHub with Actions enabled.

## Build and release trust

The local zipapp builder is deterministic: it fixes archive ordering, timestamps,
metadata, and permissions; writes through a same-directory temporary file; validates
ZIP integrity and its exact member surface; derives Name, Version, Summary,
Requires-Python, License-Expression, runtime guard, and license payload from public
project sources; executes `--version`; and atomically installs the result. Its tests
also process a public synthetic plain/HTML-related message through the built archive,
verify that the plain body alone remains while the HTML resource closure and ordinary
attachment are absent, and confirm that the source remains unchanged.

Official release construction begins only after the tag qualification matrix passes.
The build job:

1. validates the tag again against `[project].version`;
2. builds the wheel and source archive with the locked backend via
   `uv build --no-build-isolation --no-sources` twice after removing `PYTHONPATH` and
   `SOURCE_DATE_EPOCH` and enabling the strict Python runtime environment, then
   requires byte-identical outputs;
3. safely inspects without extracting: the source archive must have one root, only
   regular files, and exactly the audited public source surface plus `PKG-INFO`; the
   wheel must contain exactly the configured package sources, license, and required
   dist-info files, with regular portable members, a complete SHA-256 `RECORD`, and
   the precise `cp314-none-any` tag; `PKG-INFO` and `METADATA` must be byte-identical
   Core Metadata 2.5, the locked Hatchling 1.32.0 default; both archives must use the
   pinned backend's normalized timestamps, ownership, platform marker, and file
   modes;
4. installs and smoke-tests each distribution in an isolated environment;
5. builds and smoke-tests the deterministic zipapp;
6. generates and verifies `SHA256SUMS`;
7. transfers those exact artifacts to a separately permissioned publish job;
8. reverifies the downloaded exact four-file set, every checksum, both archive
   surfaces, source bytes, metadata, runtime requirement, license, console entry
   point, and wheel compatibility tag against the matching checkout;
9. creates GitHub provenance attestations for the three checksummed artifacts and a
   separate attestation for `SHA256SUMS` itself;
10. peels the live tag and requires it still targets the exact triggering commit;
11. requires a regular, non-symbolic release-notes file named for that exact tag,
    checks its heading against the version-derived release title, and publishes that
    reviewed body only from the existing tag matching `[project].version`.

`gh release create --verify-tag` checks tag existence; the separate API check prevents
a moved or replaced tag from redirecting the release after qualification. This
project does not claim a cryptographic signed-tag policy. The CPython-specific wheel
tag prevents other implementations from treating the wheel as compatible, and its
installed command checks CPython 3.14 before importing the MIME-processing
implementation.

The synchronized release environment supplies the exact Hatchling version and
locked transitive dependencies before build isolation is disabled. Ordinary external
PEP 517 builds remain isolated and continue to use `[build-system]`.

Local `build/`, `dist/`, and `release-dist/` contents are ignored generated data and
carry no release trust. A checksum detects changed bytes; a GitHub attestation also
connects the asset digest to this repository's release workflow.

## Platform boundary

The Finder/Shortcuts installer and launcher are POSIX shell-checked and tested where
`/bin/sh` is available. Creating the **Create Text-Only EML Copy** action, exercising
it on disposable copies of the external field corpus, opening the generated EML, and
confirming Finder's reveal behaviour remain manual integration checks. Private field
messages, filenames, payloads, and observations are never copied into the repository
or uploaded as evidence.

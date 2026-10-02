# Quality assurance contract

The product contract is MIME structure and source-byte fidelity, not visual equivalence.
Public tests use only synthetic messages; field EML files, paths, hashes, screenshots,
and observations stay outside this repository and public CI artifacts.

## Required local gates

UV 0.12.21 is required by `pyproject.toml`; CI and the digest-pinned local Linux image use that version. Direct tool dependencies and the transitive lockfile are current as of 2026-10-01. Pyflakes 4 and Setup UV 10 are included; workflow caches use Setup UV's safer `auto` default, with release asset build/publish caches disabled. Hatchling's build-system and development pins agree, and pytest's required Hypothesis plugin is checked against its development dependency pin.

Signal-tracing tests resolve the implementation selected by the pinned Mutmut trampoline, including its process-local selector, rather than tracing a decorator's unused code object. Tests verify original, statistics, selected-mutant, and unrelated-mutant dispatch while actual calls continue through the trampoline. Shutdown-operation injection wraps the operation and preserves its selected implementation.

During mutation testing, ordinary test subprocesses inherit the generated application source and workspace import paths; installed-artifact tests explicitly remove that override. A regression test verifies parent/child source agreement. A test-only startup hook initializes the child's Mutmut configuration independently of its working directory and journals distinct measured call names; the parent attributes those names to the test before collecting its mapping, including calls preceding immediate process exit. Unexpected in-process hard exits fail the test without killing its mutation worker; actual process-exit behavior runs in exec children. Finite-output mutations have a generous per-test deadline so nontermination becomes a test failure before the runner timeout. Accepted output is checked as it arrives, and real pipe readers stop if output exceeds the expected payload.

CPython 3.14.8 was published on 2026-09-30, but the current Astral managed-build and GitHub Python-version catalogs do not yet provide it. An actual UV installation request fails; the six qualification lanes retain installable 3.14.7/3.14.7t. Python 3.15 is still prerelease. Update the interpreter pin, local plan and all workflow lanes together once the provider offers both standard and free-threaded builds; a consistency test checks the local interpreter pins against `.python-version`.

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

A mutation campaign owns its checkout: `tasks.py mutation` (and the containerized
campaign of `tasks.py ci`) takes an exclusive lease on `build/mutation.lock` and a
second run in the same checkout fails at once, naming the holder's pid, instead of
sharing `mutants/` and `build/`. Whatever the outcome, the campaign leaves
`build/mutation-diagnostics/`: an `index.json` with every mutant that was not killed
(status and mapped tests) and a bounded patch for each (at most 100 mutants, 16 KiB
per patch, 25 tests each, with truncation recorded); the mutation workflow uploads
it with the results. `quality --native` runs only what depends on the host (the
repository audit and coverage); shared static checks run on the one lane CI marks
`static`, explicitly invoking mypy for `linux`, `darwin`, and `win32`.
`tasks.py ci` mirrors that split.

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

## Report lifecycle evidence

One report has one owner (`report_session.py`): render into private staging, seal it
(flush, rewind, and read bytes from each file), deliver, release the spools, then
diagnose. Retained receipt ownership lasts through delivery. Staging failures are
handled before the first external byte.
The CLI stores the authoritative ledger before execution can publish. Batch
execution operates on that same object, so there is no return/assignment handoff
that can discard results. `test_owned_cancellation.py` sends real SIGINT at
reservation, inventory, publication, finalization, final archiving, return, and
report-entry boundaries for all three formats.

Signal handlers record requests without raising through assignments. Explicit
checkpoints surround long processing and the publication visibility boundary;
receipt transfer and immutable terminal-outcome commitment complete before the
next checkpoint. `test_coherent_outcome_signals.py` sends real SIGINT before
visibility, before the publisher returns, after receipt transfer, and during
terminal commitment in every format. It checks actual copies, unchanged originals,
decoded bodies, hashes, verified receipts, terminal completeness, and one stderr
notice for non-JSON reports. Unresponsive processing has a 10-second escape and
repeated signals terminate it immediately. Report preflight rejects unfinished
rows before external output.

The controller design and its separate counterexample review cover partial handler
installation, failed thread construction/start (including failure after launch),
join failure, shutdown signals, nesting, and copied thread contexts. One owner
prepares resources before context publication. Its independent releases stop/wake
the monitor, join only a launched thread, and reset the context while cooperative
handlers remain installed; handler restoration follows. Primary errors retain
cleanup failures as exception notes. Shutdown requests are acknowledged even
after context reset. `test_controller_lifecycle.py` verifies the decisive second
real API call in the same interpreter after failed setup or interrupted teardown,
including SIGINT at the proven publisher return. It also verifies monitor ownership
and explicit wakeup independently of a machine-specific latency threshold.

A local three-round comparison used the exact audited source and current source,
verified import locations, 35 real small-message dry runs per round, and five
warm-up exclusions. The audited median was 55.2–55.4 ms; the corrected median was
2.6–2.7 ms. These measurements establish removal of the polling floor on that host,
not a universal runtime target. No signal handler invokes synchronization or thread
management operations.

The cancellation-finalization design has a separate review of scope-exit catches,
handler handback, forwarded requests, already delivered JSON, and duplicate notices
across guard/controller retirement. The ledger-aware catch covers acquisition,
processing, and controller exit. Final acknowledgement reads explicit retired state
after handler restoration; delivery requests are consumed only when its retired
guard selects status. `test_cancellation_finalization.py` sends real SIGINT during
stop, join, context reset, and processing/delivery handler restoration. It requires
completed API return values, unchanged originals, verified copies and hashes,
truthful post-publication failures, stable complete report bytes, status 130, and
one interruption explanation. It also covers early JSON error delivery and a
request at both retirement boundaries in one invocation.

The CLI-only diagnostic design and its separate review cover interruption after
JSON error-handler retirement, actual stdout acceptance, failed notice production,
blocked stderr, and caller-owned API signal policy. `_RunState` retains the error
delivery guard as acceptance evidence. An outer CLI catch includes error responses
and release; it never restarts accepted stdout and contains notice failures without
recursive rendering. A fresh diagnostic guard carries the known request into the
deadline and avoids counting an enclosing owner's same request twice.
`test_cli_outer_interruption.py` uses real SIGINT after usage and temporary-root
error delivery and compares the original JSON bytes. It covers pre-delivery and
partial output, failed writes/formatting, and enclosing owners.
`test_cli_interruption_notice_process.py` proves grace and repeat termination
against an actual blocked diagnostic worker. API cancellation ownership is unchanged.

The output-finalization design and its separate review distinguish the selected
`SystemExit` status from the exit observed after interpreter cleanup. Linux
`/dev/full` subprocess tests exercise buffered and unbuffered stdout and stderr,
recording selection 130 separately from actual 120/130, complete versus unavailable
JSON, and original/copy integrity. Status 120 is a secondary failure, not an
application success or a replacement for the processing evidence. Finder accepts
this status discrepancy only after its existing report validation, displays the
available outcomes with an explicit failure notice, retains exit 120, and suppresses
reveal. Invalid/truncated reports remain rejected. No stream ownership or shutdown
policy is imposed on API callers.

Both receipt spools retain OS-owned temporary handles throughout delivery and
never reopen a pathname. POSIX backing storage is anonymous or unlinked at
creation; Windows uses the stdlib delete-on-close handle contract. Reads remain
bounded, and a failed append rolls back to its prior byte count.
`test_spool_process_lifetime.py` exercises real processes on ordinary completion,
grace expiry, repeated cancellation, and broken pipes, requiring no orphaned files.
`test_raw_format_order.py` checks final-choice inference, known option-value
consumption, negative-number values, and end-of-options boundaries.
A failed flush, rewind, first read, write, or reset falls back to retained terminal
receipts using memory channels, with a 128 MiB limit per channel and no spool reads
or writable-storage dependency. `test_report_recovery.py` covers persistent faults,
already active recovery, cancellation during memory staging, native bytes before
and after decoder read-ahead distances,
and every output format. Recovery JSON stays canonical ASCII regardless of terminal
codec. Output endpoint failures can still prevent completion. A cleanup failure is diagnosed
after the report instead of replacing it. Delivery runs on a helper thread watched by a
`DeliveryGuard`: a signal is recorded, the first one grants a 10-second grace since
the last accepted output, a second signal or an expired grace calls `os._exit(130)`.
`test_delivery_process.py` proves this against a real pipe whose reader never reads.
A signal that arrives during delivery leaves the whole document unchanged and exits
130; the Finder launcher accepts that disagreement. Signals to the launcher itself
forward cancellation, wait at most 12 seconds for the processor (a repeated signal
kills it immediately), validate and display available complete results, suppress
reveal actions, then remove temporary reports and preserve the launcher's signal
status. Invalid or truncated reports remain rejected. `test_finder_cancellation.py`
tests the wrapper and child separately, including uncooperative children.

Delivery checks accepted counts and retains unwritten bytes across partial writes,
zero/None progress, and nonblocking write or flush failures. Only accepted output
resets the cancellation grace. `test_report_delivery_writes.py` covers a real raw
nonblocking POSIX pipe and partial buffered acceptance.

Report admission (`report_budget.py`) serializes each item with a worst-case terminal
form (longest reportable address, status, phase, error) and reserves every later
input's minimal record; `test_report_budget.py` checks the reservation against
generated real terminal records and the capacity arithmetic to the byte. The
destination address bound is enforced at destination binding, before any copy.
Diagnostics have both a 2,048-character limit and a 12,288-byte canonical ASCII JSON
content limit, including the truncation marker. `test_report_diagnostic_budget.py`
checks controls, Unicode, non-BMP characters, escaping, truncation boundaries, and
cumulative reservations.

## MIME and raw-wire evidence

`test_live_delivery_boundary.py` processes real synthetic messages and checks
first-delivery-read recovery, separate stderr progress, partial-output refusal to
restart, and broken endpoints. Batch errors appear once on non-JSON stderr.
`test_early_report_intent.py` covers allocation failures, unsafe temporary roots,
usage errors, option boundaries, startup cancellation, and ASCII JSON on hostile
terminal codecs. The error renderer uses bounded memory, not temporary files.

`test_public_api_result.py` checks repeated apply, dry-run, failed, and existing-file
calls, detached complete evidence, cleanup on exceptional unwind, and the
prepublication evidence limit. Retention is restricted to single-file results.
`test_physical_header_names.py` covers RFC 5322 punctuation in first/later positions,
exact source spans, all byte boundaries, and malformed structured MIME controls.

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

## Native macOS presentation qualification

The macOS quality lanes compile the presentation model and build a fresh locally signed universal app, verify its complete bundle signature and both architectures, and exercise its bundled launcher against a synthetic EML. POSIX quality lanes also lint the native integration shell scripts. Native model checks cover created, existing-verified, stopped, unprocessed, failed, published-with-error, and planned results; invocation status 120 or late interruption cannot become success merely because file receipts succeeded. Stopped and unprocessed rows retain their diagnostics without inflating the processing-error badge.

Before release, live-test the existing Finder Quick Action and direct Terminal launcher with public synthetic fixtures. Check successful attachment removal, exact existing-copy verification, conflicts, mixed batches, long and control-character filenames, warning display, selectable Details and complete Copy details, Finder reveal, keyboard Done, and repeated invocation while earlier reports remain open. Hold a real processor after several publications, activate Stop processing in the native UI, resume it to receive interruption, and compare the complete receipt with actual files and owned PID exits. Separately test a processor that cannot cooperate with cancellation, late interruption, output-finalization failure, missing runtime, and invalid reports; clearly identify injected fault cases. Repeat affected checks after remediation and retain the methodology and receipts in private persistent storage outside the checkout. Live GUI qualification is distinct from automated Python coverage and does not imply qualification on other macOS versions or Intel hardware.

## Complete macOS release delivery

The v4 publisher requires the prebuilt universal macOS ZIP in addition to the portable artifacts. Both production and downloaded-artifact verification run on macOS through `python -B -m tools.release_delivery`; a portable-only qualification cannot publish a v4 release. The [release pipeline contract](integrations/macos-ui/RELEASE.md) documents the five-file set, independent builds, archive permissions and extraction, source checks, ad-hoc signing, installation, and publication boundaries.

## Swift quality and exception governance

On macOS, run `uv run /bin/sh integrations/macos-ui/install-quality-tools.sh` once, then `uv run python tools/tasks.py quality`. The same quality task runs the Swift gate in both standard and free-threaded native lanes. `EML_SWIFT_TOOLS` optionally selects an external versioned tool directory; tool caches and build products never belong in the checkout or application. The tool versions, upstream revisions, installer signer, and release checksums are authoritative in `integrations/macos-ui/toolchain.toml`.

Apple's swift-format owns canonical formatting and naming, using `integrations/macos-ui/swift-format.json`; lint uses `--strict` and never ignores unparsable files. SwiftLint owns the complementary correctness and structural rules selected in `integrations/macos-ui/swiftlint.yml`; both its configuration and invocation are strict, with no lint cache. Warning diagnostics fail unless an exact, centrally reviewed waiver applies. Source discovery includes every Swift file under the native integration, including model tests and artwork generators. Real negative controls verify formatting/parse failures, unsafe force-try, file/type/function size, nesting, and cyclomatic complexity.

`lint-exceptions.json` is the central approval registry for narrow Ruff/mypy source directives and SwiftLint waivers. Python compiler/linter directives stay at their required source locations, but approvals bind the exact file, enclosing declaration, source anchor and rules to a rationale. The registry gate rejects unregistered, stale, duplicate and reasonless approvals; blanket Python directives and inline Swift suppression commands are forbidden. SwiftLint waivers identify an exact source anchor and rule, and must correspond to a current diagnostic. No Swift waivers are currently needed. Broader Python configuration policy remains in `pyproject.toml`: Ruff selects ALL with explicit formatter/licensing policy exclusions and narrow test/tool policies; mypy is strict with additional checks. Review an exception's actual necessity rather than assuming a registry entry proves its quality.

Python structural checks discover source, test and tool modules and enforce physical/substantive line budgets, top-level declaration counts, and class-method budgets, including nested classes. Ruff separately limits Python complexity, branches, arguments, locals, returns, statements and nesting. SwiftLint enforces file/type/function length, arguments, nesting and cyclomatic complexity. These are architecture heuristics rather than proofs of cohesive responsibilities; reviewers must also consider types spread across extensions and responsibilities shared between files. The native UI composes process ownership, runtime configuration, window lifecycle, layout, file selection/reveal, and diagnostic controls as separate components.

## Native receipt fuzzing

The separate `Native receipt fuzzing` GitHub workflow uses the official Swift.org compiler pinned by version, download hash and installer signer in `integrations/macos-ui/toolchain.toml`. The Apple Swift 6.4 compiler tested with our standalone Command Line Tools rejects `-sanitize=fuzzer`. Production builds, model tests, SourceKit linting, and fuzzing use this same compiler toolchain. Apple Command Line Tools or Xcode supply the macOS SDK and platform/signing tools; neither development compiler nor fuzz harness is bundled in the app. The Swift fuzz sources remain subject to the same strict formatting, lint and structural gates as the native app.

Run `uv run /bin/sh integrations/macos-ui/install-swift-toolchain.sh` once on macOS, then `uv run /bin/sh integrations/macos-ui/fuzz.sh "$HOME/Library/Application Support/EML Attachment Remover QA/receipt-fuzz"`. The runner compiles the actual report model with libFuzzer and AddressSanitizer, explicitly selects the platform SDK, replays every checked-in seed, then explores a writable copy of the corpus outside the checkout. Campaign defaults are authoritative in `integrations/macos-ui/fuzzing.toml`. The default campaign requests 100,000 executions with seed 1, a 64 KiB input limit, a 10-second per-input timeout and a 2 GiB RSS limit. These are exploration limits, not changes to the processor's report capacity. A fixed seed aids reproduction but does not guarantee identical exploration across machines or compiler environments.

The macOS `tools/tasks.py ci` plan also installs the shared toolchain and runs the standard campaign, preserving its outputs under the private application-support QA directory. PRs and main pushes run the same target with an additional five-minute campaign limit. Manual workflow dispatch can select an extended 30-minute campaign. Longer local exploration uses the same command with `-runs=-1 -max_total_time=1800`; replay a saved failure with `receipt-fuzzer /absolute/path/to/crash-input`. The output directory retains invocation metadata, replay/campaign logs, discovered corpus inputs and libFuzzer failure artifacts. CI uploads these even after a failed campaign and retains them for 30 days; preserve valuable failures outside CI before retention expires. Minimize and review useful cases before adding them to the checked-in synthetic corpus; do not commit private message contents.

The target exercises JSON decoding, receipt version/process-status admission, all presentation properties, native-byte and text addresses, NUL rejection, unsafe display characters, unsuccessful invocation classification and verified-output reveal restrictions. Malformed JSON is an expected rejection. The Swift model consumes receipts already validated by the private launcher; this harness does not establish full schema admission at that launcher boundary. It performs no file publication or UI actions. Memory sanitization and these explicit invariants supplement Python property/mutation tests and live Finder, window, cancellation and process-lifetime qualification; they do not replace them. Foundation and operating-system libraries are not rebuilt with our coverage instrumentation.

The optional extended workflow has this direct local equivalent:

```sh
FUZZ_OUTPUT="$HOME/Library/Application Support/EML Attachment Remover QA/receipt-fuzz-extended"
uv run /bin/sh integrations/macos-ui/fuzz.sh "$FUZZ_OUTPUT" -runs=-1 -max_total_time=1800
```

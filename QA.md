# Quality assurance contract

The product contract is MIME structure and source-byte fidelity, not visual equivalence.
Public tests use only synthetic messages; field EML files, paths, hashes, screenshots,
and observations stay outside this repository and public CI artifacts.

## Required local gates

Development checks require Git 2.28 or later for real-history verification; the local Linux runner image includes Git. Packaged CLI and app users do not need Git. From a clean checkout, run `uv sync --locked --group dev`. On macOS, also install the pinned Swift toolchain and quality tools using the commands below; complete native release production requires full Xcode as described in the [macOS build guide](integrations/macos-ui/README.md#build-from-source). The dependency restore alone does not install those native tools.

UV 0.12.23 is required by `pyproject.toml`; CI and the digest-pinned local Linux image use that version. Dependency declarations and build-system pins are authoritative in `pyproject.toml`, resolved dependencies in `uv.lock`, and native tool pins in `integrations/macos-ui/toolchain.toml`. Workflow caches use Setup UV's `auto` setting, with release asset build/publish caches disabled. Hatchling's build-system and development pins agree, and pytest's required Hypothesis plugin is checked against its development dependency pin.

Signal-tracing tests resolve the implementation selected by the pinned Mutmut trampoline, including its process-local selector, rather than tracing a decorator's unused code object. Tests verify original, statistics, selected-mutant, and unrelated-mutant dispatch while actual calls continue through the trampoline. Shutdown-operation injection wraps the operation and preserves its selected implementation.

During mutation testing, ordinary test subprocesses inherit the generated application source and workspace import paths; installed-artifact tests explicitly remove that override. A regression test verifies parent/child source agreement. A test-only startup hook initializes the child's Mutmut configuration independently of its working directory and journals distinct measured call names; the parent attributes those names to the test before collecting its mapping, including calls preceding immediate process exit. Unexpected in-process hard exits fail the test without killing its mutation worker; actual process-exit behavior runs in exec children. Finite-output mutations have a generous per-test deadline so nontermination becomes a test failure before the runner timeout. Accepted output is checked as it arrives, and real pipe readers stop if output exceeds the expected payload.

Qualification uses CPython 3.14.8 from `.python-version`, with matching standard/free-threaded versions in local and workflow lanes. Pinned UV 0.12.23 offers both variants across the supported CI platforms; the earlier installer-catalog delay has been resolved. The runtime contract remains CPython 3.14, and the bundled macOS runtime is separately integrity-pinned. Python’s macOS 27 Tk/IDLE dialog notice does not describe this AppKit interface: the application does not use Tkinter. This distinction does not replace live OS qualification.

Run every CI gate this host can reproduce before pushing:

```sh
uv run python tools/tasks.py ci
```

`ci` installs both CI interpreters (CPython 3.14.8 and free-threaded 3.14.8t) into
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

On POSIX, every task command runs in its own process group. A timeout or Ctrl-C sends the
group SIGINT, then SIGKILL after a grace period; after a normal exit, leftover group
members are killed. No descendant can write evidence once its task step returns.
Windows task timeouts stop the direct child; this runner does not guarantee descendant cleanup there. Mutmut requires a fork-capable POSIX runtime.

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
evidence. `--workers auto|1-64` selects Mutmut parallelism. On an overloaded host, test deadlines can turn an equivalent mutant into a misleading kill or interrupt a check. Re-run disputed IDs with fewer workers in an isolated environment and validate the complete named results before accepting the score.

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

No signal handler invokes synchronization or thread-management operations. The wakeup and monitor-ownership tests establish controller behavior; they do not promise a machine-independent runtime or latency target.

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

## Controller and report implementation boundaries

The invocation controller prepares its monitor before binding its context, restores that context independently of cleanup failures, and keeps cooperative handlers installed through shutdown. Normal shutdown wakes and joins the monitor immediately, without a polling delay. Nested scopes in the same thread share their active owner; a copied context in another thread acquires its own owner. The API catches cooperative requests through controller shutdown and returns its completed ledger with invocation interruption metadata. Final checks inspect each controller only after its handler stops accepting requests. Delivery status is selected from the retired guard; a late request preserves report bytes and receives one interruption explanation. The CLI's outer interruption boundary also covers error-response finalization and resource release after handlers retire. A complete or partial error document is never replaced; late interruption returns 130 with one bounded stderr notice. A blocked notice retains the cancellation deadline and repeat-signal escalation.

Signal tests set and restore a controlled SIGINT baseline independently of the invoking shell; tests of explicitly ignored signals still install SIG_IGN within their own scope. Arbitrary byte and seed-splice MIME properties run under the selected quality/thorough profile; expected syntax or role rejections remain AppError, while accepted results must satisfy candidate verification. These input-space checks supplement branch coverage and mutation tests rather than treating any one of them as exhaustive.

## MIME and raw-wire evidence

Native launch checks exercise the actual ProcessingRun wrapper and POSIX launch-error explanation. Selections travel through a four-byte-length-prefixed native-path request, so the processor launch uses a fixed argument list rather than one argument per filename. Tests exercise 4,096 inputs, invalid paths, malformed frames and real OS argument-byte errors for command-line launches. Actual owner EOF is checked in processing and delivery scopes; an independent non-consuming macOS pipe observer supervises a suspended processor. Forced exit can prevent a complete report and leave a named candidate stage, as documented above.

The verifier authorizes source removals separately from the planner and independently constructs the expected edited bytes; forged-removal tests require it to reject unauthorized body deletion even when the candidate remains parseable. This is independence of policy and edit checks, not a fully independent source parser: the planner and verifier share the source RawNode tree, and candidate parsing uses the same raw parser. The standard-library structural cross-check supplies another view but does not prove correctness for every malformed input. Keep the separate authorization predicates rather than making a planner mistake self-consistent through shared decision helpers.

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
protocol locally. The release workflow must gate publication on successful
quality, mutation, archive, checksum, provenance, and artifact-identity jobs.

## Native macOS presentation qualification

The current macOS producer lane compiles the presentation model and builds fresh locally signed single-CPU apps, verifies their complete bundle signatures and exact architectures, and exercises their bundled launcher against a synthetic EML. Older-OS lanes run the downloaded matching app without rebuilding its icon. Both pull-request and tagged-release compatibility gates run the transferred candidate on macOS 14 Apple Silicon, macOS 15 Intel and macOS 27 Apple Silicon; release publication waits for the tagged build's own consumers. The macOS 14 lane additionally runs the Intel model and downloaded Intel app through Rosetta on that minimum OS. Translation does not replace a physical macOS 14 Intel test. POSIX quality lanes also lint the native integration shell scripts. Native model checks cover created, existing-verified, stopped, unprocessed, failed, published-with-error, and planned results; invocation status 120 or late interruption cannot become success merely because file receipts succeeded. Stopped and unprocessed rows retain their diagnostics without inflating the processing-error badge.

Before release, live-test the existing Finder Quick Action and direct Terminal launcher with public synthetic fixtures. Check successful attachment removal, exact existing-copy verification, conflicts, mixed batches, long and control-character filenames, warning display, selectable Details and complete Copy technical report, Finder reveal, keyboard Done, and repeated invocation while earlier reports remain open. Hold a real processor after several publications, activate Stop processing in the native UI, resume it to receive interruption, and compare the complete receipt with actual files and owned PID exits. Separately test a processor that cannot cooperate with cancellation, late interruption, output-finalization failure, missing runtime, and invalid reports; clearly identify injected fault cases. Repeat affected checks after remediation and retain the methodology and receipts in private persistent storage outside the checkout. Live GUI qualification is distinct from automated Python coverage and does not imply qualification on other macOS versions or Intel hardware.

## Complete macOS release delivery

The v4 publisher requires both runtime editions for both macOS CPUs in addition to the portable artifacts. Both production and downloaded-artifact verification run on macOS through `python -B -m tools.release_delivery`; a portable-only qualification cannot publish a v4 release. The [release pipeline contract](integrations/macos-ui/RELEASE.md) documents the eight-file set, independent builds, archive permissions and extraction, source checks, ad-hoc signing, installation, and publication boundaries.

Native layout checks use production views with the optimized compiler to exercise the batch table at its 560-point minimum width and at 1,000 points: file names gain the extra space, result text fits, and result tooltips describe the result. Native artwork checks compile and run the review exporter and inspect PNG dimensions and sRGB metadata. Its icon study illustrates geometry; actual compiled icons, materials and appearance variants still need visual qualification in Icon Composer and on supported operating systems.

## Swift quality and exception governance

On macOS, run `uv run /bin/sh integrations/macos-ui/install-quality-tools.sh` once, then `uv run python tools/tasks.py quality`. The same quality task runs the Swift gate in both standard and free-threaded native lanes. `EML_SWIFT_TOOLS` optionally selects an external versioned tool directory; tool caches and build products never belong in the checkout or application. The tool versions, upstream revisions, installer signer, and release checksums are authoritative in `integrations/macos-ui/toolchain.toml`.

Apple's swift-format owns canonical formatting and naming, using `integrations/macos-ui/swift-format.json`; lint uses `--strict` and never ignores unparsable files. SwiftLint owns the complementary correctness and structural rules selected in `integrations/macos-ui/swiftlint.yml`; both its configuration and invocation are strict, with no lint cache. Warning diagnostics fail unless an exact, centrally reviewed waiver applies. Source discovery includes every Swift file under the native integration, including model tests and artwork generators. Real negative controls verify formatting/parse failures, unsafe force-try, file/type/function size, nesting, and cyclomatic complexity.

`lint-exceptions.json` is the central approval registry for narrow Ruff/mypy source directives and SwiftLint waivers. Python compiler/linter directives stay at their required source locations, but approvals bind the exact file, enclosing declaration, source anchor and rules to a rationale. Standalone and closing-line directives bind to the owning statement or declaration rather than empty text or punctuation. The registry gate rejects unregistered, stale, duplicate and reasonless approvals; blanket Python directives and inline Swift suppression commands are forbidden. SwiftLint waivers identify an exact source anchor and rule, and must correspond to a current diagnostic. No Swift waivers are currently needed. File-wide Ruff policies must be declared in `pyproject.toml`; inline file-ignore directives are rejected. Broader Python configuration policy remains in `pyproject.toml`: Ruff selects ALL with explicit formatter/licensing policy exclusions and narrow test/tool policies; mypy is strict with additional checks. Review an exception's actual necessity rather than assuming a registry entry proves its quality.

Python structural checks discover source, test and tool modules and enforce physical/substantive line budgets, top-level declaration counts, and class-method budgets, including nested classes. Ruff separately limits Python complexity, branches, arguments, locals, returns, statements and nesting. SwiftLint enforces file/type/function length, arguments, nesting and cyclomatic complexity. These are architecture heuristics rather than proofs of cohesive responsibilities; reviewers must also consider types spread across extensions and responsibilities shared between files. The native UI composes process ownership, runtime configuration, window lifecycle, layout, file selection/reveal, and diagnostic controls as separate components.

## Native receipt fuzzing

The separate `Native receipt fuzzing` GitHub workflow uses the official Swift.org compiler pinned by version, download hash and installer signer in `integrations/macos-ui/toolchain.toml`. The Apple Swift 6.4 compiler tested with our standalone Command Line Tools rejects `-sanitize=fuzzer`. Production builds, model tests, SourceKit linting, and fuzzing use the pinned official Swift.org compiler toolchain. Apple Command Line Tools or Xcode supply the macOS SDK and platform/signing tools; neither development compiler nor fuzz harness is bundled in the app. The Swift fuzz sources remain subject to the same strict formatting, lint and structural gates as the native app.

Run `uv run /bin/sh integrations/macos-ui/install-swift-toolchain.sh` once on macOS, then `uv run /bin/sh integrations/macos-ui/fuzz.sh "$HOME/Library/Application Support/EML Attachment Remover QA/receipt-fuzz-$(date +%Y%m%d-%H%M%S)"`. The runner compiles the actual report model with libFuzzer and AddressSanitizer, explicitly selects the platform SDK, replays every checked-in seed, then explores a writable copy of the corpus outside the checkout. Campaign defaults are authoritative in `integrations/macos-ui/fuzzing.toml`. The default campaign requests 100,000 executions with seed 1 and a five-minute wall-clock limit, a 64 KiB input limit, a 10-second per-input timeout and a 2 GiB RSS limit. These are exploration limits, not changes to the processor's report capacity. A fixed seed aids reproduction but does not guarantee identical exploration across machines or compiler environments.

The macOS `tools/tasks.py ci` plan also installs the shared toolchain and runs the standard campaign, preserving its outputs under the private application-support QA directory. PRs and main pushes run the same target with the same five-minute campaign limit; tagged release builds require this same reusable workflow to pass against the tagged source. Manual workflow dispatch can select an extended 30-minute campaign. Longer local exploration uses the same command with `-runs=-1 -max_total_time=1800`; replay a saved failure with `receipt-fuzzer /absolute/path/to/crash-input`. Use a fresh output directory for every campaign; existing paths, including symlinks, are refused without overwriting evidence. Only `-runs` and `-max_total_time` overrides are accepted, so artifact locations and input/resource limits remain governed by the configuration. Compilation has a five-minute deadline, seed replay a one-minute deadline, and the campaign a deadline equal to its requested duration plus 30 seconds for shutdown. Timeout, Ctrl-C and SIGTERM clean the owned process group. The private output directory retains copied sources and regression seeds, their SHA-256 digests, compiler/SDK identity, stage commands and outcomes, compile/replay/campaign logs, discovered corpus inputs and libFuzzer failure artifacts. Metadata is written before compilation or replay, so failures in those stages remain identifiable. CI uploads these even after a failed campaign and retains them for 30 days; preserve valuable failures outside CI before retention expires. Minimize and review useful cases before adding them to the checked-in synthetic corpus; do not commit private message contents.

Each input also creates a bounded structured receipt, so malformed JSON mutations still exercise presentation and outcome branches. Rendered strings and returned URLs have explicit assertions rather than discarded results; deliberate broken-model controls must fail under the optimized build, and a real heap-overflow control verifies ASan detection. The target exercises JSON decoding, receipt version/process-status admission, all presentation properties, native-byte and text addresses, NUL rejection, unsafe display characters, unsuccessful invocation classification and verified-output reveal restrictions. Malformed JSON and invalid UTF-8 receipt bytes are expected rejections. On macOS 14, the system JSON decoder can terminate on invalid UTF-8 before returning an error; the model validates the byte sequence first. The regression uses the exact four-byte corruption found by hosted fuzzing. The Swift model consumes receipts already validated by the private launcher; this harness does not establish full schema admission at that launcher boundary. It performs no file publication or UI actions. Memory sanitization and these explicit invariants supplement Python property/mutation tests and live Finder, window, cancellation and process-lifetime qualification; they do not replace them. Foundation and operating-system libraries are not rebuilt with our coverage instrumentation. The local macOS 27 heap-overflow control also emits an upstream sanitizer warning about an invalid dyld module map: detection succeeds, but that warning limits confidence in backtraces. Retain the full diagnostic and source snapshot rather than suppressing the warning. Production uses Swift 6 language mode, warnings as errors, checked `-O` optimization and explicit macOS 14 targets for both CPU slices; it does not use `-Ounchecked` or sanitizers. Model contracts run with both `-Onone` and `-O`, and bundle tests reject libFuzzer/ASan runtime symbols and dependencies. The selected SDK is supplied by the platform tools, not pinned by the Swift package; reproducibility claims apply to identical build environments.

The optional extended workflow has this direct local equivalent:

```sh
FUZZ_OUTPUT="$HOME/Library/Application Support/EML Attachment Remover QA/receipt-fuzz-extended-$(date +%Y%m%d-%H%M%S)"
uv run /bin/sh integrations/macos-ui/fuzz.sh "$FUZZ_OUTPUT" -runs=-1 -max_total_time=1800
```

## Report staging and recovery mechanisms

Each report is staged in the requested format and mode and validated before delivery. Machine-readable sealing uses bytes throughout, including native POSIX filenames. If staging fails after a visible publication, retained terminal receipts produce a complete recovery report in bounded memory, independently of staging files and spool reads, in the same byte format. A staged read failure before stdout accepts its first report byte can also recover through fresh memory channels. Stderr progress is tracked separately; already emitted diagnostics are not repeated. After any report bytes have been accepted, a failure never restarts or appends a replacement document. Receipt storage stays owned until delivery resolves. Each recovery channel is limited to 128 MiB. The CLI owns its authoritative ledger before any input can publish; cancellation during reservation, inventory, final archiving, or transition to reporting retains every known outcome. Receipt spools use OS-owned temporary handles: anonymous/unlinked-open storage on POSIX and delete-on-close storage on Windows. Forced termination does not depend on Python cleanup to remove them. Human and `paths0` output explain batch failures once on stderr while retaining truthful successful item statuses and paths. Delivery still depends on an available output endpoint; a closed pipe or channel error can prevent completion. Signals record cancellation requests; explicit checkpoints acknowledge them before publication or after a complete publication outcome is committed. A proven copy keeps its successful receipt, and genuine post-publication errors keep their error status. The API returns completed item evidence with invocation interruption metadata. A late signal preserves any report bytes already delivered and produces one bounded interruption explanation. Complete or partial error documents are never replaced. The public API keeps the caller's signal policy outside its owned controller. Unresponsive processing is bounded to 10 seconds after the first signal; a second signal ends it immediately. An interruption before delivery reports the interruption and exits 130. Human and `paths0` output include one bounded, safely escaped invocation notice on stderr, even when all inputs completed. A signal during delivery leaves the document whole and unchanged (its `exit_code` is the processing outcome), then exits 130 with an `interrupted by SIGINT after the report was delivered` diagnostic. Delivery never waits forever for a reader that stopped draining: after the first signal the process allows 10 seconds without output progress, and a second signal ends it at once, both with status 130 and possibly a partial document, which is never followed by another. Without a signal there is no deadline, so a pager works.

Supported transfer encodings do not expand beyond their encoded input and retained leaf spans are disjoint. The 128-MiB raw-source limit therefore bounds decoded retained bytes; it does not bound peak RSS. Report records reserve their encoded-byte budget before publication and use bounded private spools during processing.

The mutation jobs use the ubuntu-24.04 hosted label to align their OS generation with the digest-pinned Ubuntu 24.04 local container. Hosted images still update and contain different packages from the local container; the label does not prove environment identity. Latest-host quality and property matrices remain deliberate compatibility probes. Native fuzz qualification uses the same selected, version-checked Xcode SDK as delivery.

Named publication staging and anonymous report storage have different lifetimes. Publication needs a same-directory name for the exclusive rename/link operation; a forced process exit can leave `.eml-remove-<32 hexadecimal characters>.tmp` containing retained message bytes. Normal cleanup owns only its exact stage and never sweeps a filename prefix. The cancellation monitor must not run filesystem cleanup: unlink can block on the stalled filesystem, and Windows delete-on-close cleanup of a stage handle after rename would target the published copy. The critical flag defers cooperative exceptions; it does not suspend hard-stop escalation. Forced termination can leave final copies without any JSON or API result. The simulated-fsync process tests exercise that reporting boundary and do not establish real kernel-stall timing.

Tagged CI qualification and publication share a full-history ancestry check against `refs/remotes/origin/main`. The GitHub release checkouts fetch all history and the tag qualification lanes enforce this before project dependency installation and their long gates. The publish job repeats the same check before artifact verification and remote attestations. The publisher independently enforces it before artifact verification or any remote write. Missing branch history, shallow checkouts and off-main commits fail closed; already-integrated historical commits remain eligible. This is a release-policy check in the checked-out code, not a substitute for repository permissions: a writer able to change tagged workflows can change their checks. The repository’s existing tag ruleset protects `v*` updates and deletion; it does not itself enforce ancestry.

The local CI plan keeps its version-only candidate check without claiming it mirrors the authoritative tagged-checkout ancestry step. That tag-context step is explicitly CI-only in the parity inventory; the same mechanism is exercised locally against real complete, shallow, integrated and unmerged Git histories. The plan still requires every other workflow command to have a local counterpart or a justified CI-only classification.


## Framed frontend input

`--request-stdin` selects one framed request instead of positional sources; mixing the two is a usage error. Stdin must be a pipe. Its first four bytes encode the unsigned big-endian UTF-8 JSON payload length, bounded to 12 MiB. The object contains exactly `encoding` and `paths`: `encoding` is `posix-bytes` on POSIX or `windows-utf16le` on Windows; `paths` is a nonempty ordered array of Base64 native addresses. Duplicate JSON keys, invalid Base64, empty/NUL addresses, more than 4,096 inputs and more than 4 MiB of cumulative filesystem-encoded path bytes are refused before processing. Ordinary report formats and processing options remain available.

Framed selections expand directories in the Python core before creating a processing ledger. Folder collection accepts ordinary `.eml` entries, skips link/reparse entries and the authoritative generated-copy suffix, and preserves explicit file intent. It uses lexical pathname deduplication and observed directory identities without merging distinct file aliases. Directory enumeration is not an atomic snapshot; observed identity changes and enumeration failures are refused, while later source binding remains authoritative. Traversal is limited to 100,000 encountered entries, 4,096 directories and 4 MiB of cumulative directory path bytes, in addition to the collected-input count and path-byte limits. No MIME copies are created while collection is incomplete.

The frontend must retain the pipe write end without sending further data until the run finishes. EOF or trailing data requests main-thread cancellation through the currently active processing handler or delivery guard, with an independent stalled-work deadline. Cleanup retires the monitor, restores borrowed descriptor flags and closes only its duplicate. The native macOS launcher additionally observes pipe closure through kqueue without consuming request bytes, so it can stop a suspended processor through its existing supervisor. Tests distinguish real EOF and process suspension from an injected Python stall. These controls do not promise a complete report or stage cleanup after a forced exit.

### Advisory processing progress

`--progress-fd` borrows a writable pipe descriptor above stdio, duplicates it, enables nonblocking writes and restores its original flags on scope exit. Tagged `EML_PROGRESS ` ASCII JSON lines have schema 1, stage `processing` or `reporting`, and integer `completed`/`total` counts; they contain no paths or message content. A failed or partial write disables further progress. Stdout and the authoritative final schema-3 report retain their normal contracts. Generic batch observers receive completed-attempt counts, excluding cancelled/unprocessed items; ordinary observer failures retire the observer without changing processing. Process controls and memory exhaustion remain effective.

The native launcher forwards the separate progress descriptor to its stderr pipe while retaining ordinary processor stderr for final diagnostics. The reader bounds line buffering, diagnostic retention, totals and monotonic progression, rejects malformed UTF-8 before system JSON decoding, and treats updates as advisory. Progress cannot enable Finder reveal or prove success. Reporting remains cancellable until the actual child exits. Tests use real closed and full non-draining pipes, observer failure, interrupted large-batch UI processing and deliberate malformed progress inputs. The local/CI receipt fuzz harness also mutates the progress stream and checks split-record admission and bounded results.

Native file selection uses the same decoder for the drop zone and Finder Services. It admits absolute local file URLs, including localhost, and refuses mixed selections containing remote authorities, credentials, ports, query/fragment components, invalid UTF-8 or NUL encodings. Actual AppKit pasteboard tests retain Unicode, spaces, newline filenames and literal percent/query/fragment filename characters. These checks establish selection admission; an automated mouse gesture is not proof of a real Finder drag.

Native build metadata runs with bytecode disabled explicitly. An execution test copies the actual metadata program and its imports into a private source checkout, removes protective parent environment variables, verifies the produced bundle identity and refuses any generated source-cache directory.

Python distribution metadata declares MPL-2.0 for the artwork-free wheel and zipapp. The source archive declares the combined software/artwork expression and marks License-Expression dynamic, because a wheel built from that source excludes the artwork. Verification permits exactly this declared header difference while requiring all remaining source/wheel metadata bytes to agree. Proprietary artwork permissions and bundled third-party notices remain in LICENSE and the app resources.

Publication receipts record completion of the operating system synchronization calls, not a guarantee against power loss or hardware failure. POSIX files and directories use fsync; macOS additionally requests F_FULLFSYNC, retaining the fsync result when the stronger operation is explicitly unsupported. Other full-flush errors fail publication or produce a post-publication error receipt. Source-path output naming uses an address obtained from the same open handle as the inventory identity; it preserves input traversal and does not use the resolved address to reopen the source. Hard links at distinct addresses and source renames can still produce distinct automatic names. Argument validation does not expand a literal tilde; interactive shells perform their own expansion before invoking the application.

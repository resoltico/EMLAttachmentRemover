# Verification

Acceptance requires every registered native, archive and GUI check at the delivered
source. Pull requests also require current merge integration and head/base
freshness. Inspect the exact source's
[CI run](https://github.com/resoltico/EMLAttachmentRemover/actions/workflows/ci.yml)
and retained artifacts for results. A local pass, another commit's result or an
archive digest cannot substitute for these gates.

## Repository checks

Run `cargo xtask check` after installing the tools in
[CONTRIBUTING.md](../CONTRIBUTING.md#start-with-rust). It retains independent
section results and logs under `target/repository-checks/run-*`, including failed
or unavailable checks. It checks Rust formatting, compilation, Clippy, tests,
rustdoc, dependency advisory/source/license policy, workflows, shell scripts and
notices. On macOS it also requires the selected Xcode for Swift formatting and
the native desktop source checks. Tool and compiler pins are authoritative in
`xtask/tools.toml` and `rust-toolchain.toml`.

`cargo xtask check-quality` separately validates shared strict lint configuration, centrally registered exceptions, authored source discovery and structural file limits. `cargo xtask check-shell` uses the same tracked/pending/shebang discovery and root strict ShellCheck policy as the complete check command. Both excluded Rust workspaces receive Clippy checks. See the [quality policy and exceptions](../CONTRIBUTING.md#checks-and-evidence) for ownership boundaries, structural limits and the distinct Swift branch metric.

Native Swift verification includes syntax-aware callable metrics and positive/negative controls using the selected Xcode's matching SwiftParser/SwiftSyntax modules. Missing modules, malformed syntax or unexpectedly empty discovery fail the check; another local Xcode does not establish pinned-toolchain qualification.

`cargo xtask native-tests --release --target TARGET --out NEW_DIR` requires the
matching native host before test compilation, compares discovered and executed
test identities/counts, and retains transcripts. macOS release selections include
GUI, worker, Darwin and Core fixture features under the pinned Xcode policy.
Windows selections include filesystem, Job and console fixtures. Debug tests
remain required independently. A passing selection establishes only the tested
profile on that host.

`cargo xtask stage-release-assets --archives DIR --out NEW_DIR` requires exactly
the eight policy-derived ordinary archives, takes bounded immutable snapshots,
checks copied bytes and writes sorted `SHA256SUMS`. Its identity record lives
outside the nine-asset set. `assembly_source` identifies the assembling checkout,
not archive build provenance. Assembly cannot qualify package contents or native
behavior; the [release workflow](release.md) binds products to qualification.

## Native picker preference observations

Full before/after property lists and typed JSON preserve Data, dates, numeric
types and presence. Nonpicker/argument-only scenarios require whole-domain
equality. Only independently observed successful native picker scenarios may
change these exact Cocoa state keys with these native types:

| Key | Required native type |
|---|---|
| NSNavPanelExpandedSizeForOpenMode | String |
| NSOSPLastRootDirectory | Data |
| NSWindow Frame GoToSheet | String |
| NSNavLastUserSetHideExtensionButtonState | Boolean |
| NSWindow Frame NSNavPanelAutosaveName | String |

Wrong types, deletions, unexpected changes, destination changes and the same
changes in nonpicker scenarios fail comparison. Results list permitted changes
and separately report application/other preference equality. The application
neither reads nor writes these OS state keys. Tests do not reset preferences or
operate on an owner's desktop to manufacture equality.

## Native test infrastructure

GUI automation runs only on fresh owned hosted runners. Each scenario retains
its actual NSWorkspace-launched app and named pasteboard, executes one selected
XCTest, verifies original and golden bytes, and requires app/runner/worker/resource
settlement. Forced app cleanup is a failure and does not establish worker
settlement. A missing result or uncertain cleanup closes further admission.

The no-universal policy applies to every shipping CLI, app and worker. Xcode may
use `lipo -extract` only on Apple's stock XCTRunner agent for the current native
CPU. The project test bundle uses native ARCHS and ONLY_ACTIVE_ARCH=YES. No
`lipo -create` or test runner/framework/bundle enters shipping archives.
`SWIFTBUILD_LAUNCH_IN_PROCESS=YES` is scoped to xcodebuild; it does not waive
survivor rejection or change saved Xcode defaults.

Native macOS builds share CPU-specific intermediates under `target/native-cargo/`.
CI cache keys include target, lock/manifests, compiler/tool/SDK policy and Cargo
configuration. Cached intermediates are unqualified: current products are copied,
frozen, signed and subjected to image, archive, extraction, behavior and GUI
checks. Qualification records and prior test results are not restored as passes.

Windows uses the bounded native controller with discovery/execution comparison,
separate transcripts, required-name sentinels and ignored-test rejection. Actual
console tests require the CLI to consume an unfinished request or produce a real
partial report before interruption; private-console delivery and complete Job
settlement are independently checked. See [process ownership](windows-process-ownership.md).

Windows final-link review consumes the linker's own COFF reproducer, binds its
input bytes/map/trace/image and compiler/linker LLVM versions, checks official
runtime references and Cargo origins, and rejects unknown inputs. A real extra
object must link successfully and then fail admission before packaging. The
reference-generation tooling is `cargo xtask windows-runtime-references --archives
DIR --out NEW_FILE`; it validates official component checksums from
`legal/windows-runtime/source.json` and does not itself qualify Windows binaries.

The manual native-panel diagnostic runs with
`gh workflow run macos-panel-diagnostic.yml --ref BRANCH -f source=FULL_SHA`.
It is diagnostic evidence; required product picker admission is separate below.

## Native and archive qualification

Normal checks include both excluded Rust workspaces, notice source/material
validation and strict selected-Xcode formatting of retained Swift bridges/tests.
`notices --write [--out NEW_DIR]` and `notices --check [--out EXISTING_DIR]` are
explicit exclusive operations. The latter validates current source/material
bindings and, when supplied, exact generated-tree bytes and source receipt.
Intel uses the existing successful same-clean-source Linux producer/consumer
route through `EML_GENERATED_LEGAL_MATERIALS`; its evidence is explicitly
`consumer_identity`, without claiming local generator execution or review quality.
See [setup](../CONTRIBUTING.md#checks-and-evidence) for prerequisites and limits.

The reusable native workflow explicitly checks out its source input: PR head H,
main source S, or merge-queue source Q. PR integration separately checks out M,
verifies its base/H parents, executes the real Linux workspace selection and
records all three identities. After both branches finish, an API read rejects a
changed PR head/base. The Rust `ci-result` admission rejects absent results,
failure, cancellation and unexpected skips; negative controls exercise each
required role. Failed app prerequisites fail the floor consumers and aggregate. Workflow wiring
alone establishes neither product qualification nor publication behavior.

On Linux and Windows, `qualify` orchestrates strict release-profile discovery
and execution, the standalone real filesystem probe, native CLI build, complete
materials, final-runtime review, extraction and behavioral smoke. Linux also runs
the real different-UID namespace control; Windows release tests include the
distinct-account, Job and console cases, followed by an actual extra-object
rejection. On macOS,
`qualify --target <native-target> --icons <validated-icon-directory> --out <new-directory>`
reuses the same release-suite discovery, owned exFAT observation and complete app
archive builder. It requires a fresh hosted runner before native operations,
preserves the validated same-source shared icon producer and emits one
native-candidate record for the CLI and nested app archive. CI's app job owns these
release/volume checks; the separate native matrix retains debug, desktop,
formatting and namespace checks. macOS15 consumers exercise exactly the producer
archives. Producer verification ends after extraction, structure, signature,
CPU/minimum policy, actual Core-backed worker and unchanged-bundle checks; its
`native_candidate` receipt contains no successful/default GUI fields.
`smoke-macos-app` on each macOS15 consumer requires the full workflow, pagination,
real-Core publication/lifecycle cases and final appearance checks before emitting
`desktop_qualification`. Appearance changes use the owned native Settings controls,
with explicit contrast/transparency values before, during and after the scenario.
The retained Settings process is navigated through its native View commands and
Accessibility Display link, with actual target controls acknowledging each pane.
Navigation first acknowledges an already visible target pane through its native
window and identifying control. Repeated readback does not require rediscovering
a parent-page link after the target page is open.
Successful LaunchServices URL delivery alone does not establish navigation. No defaults
setter or preference-file edit substitutes for native interaction. The flow
follows [Apple's macOS15 Display navigation](https://support.apple.com/guide/mac-help/change-display-settings-for-accessibility-unac089/15.0/mac/15.0).
The actual product must independently report its unoverridden NSAppearance/
NSWorkspace observations for Light, Dark and Increase Contrast; physical frames
and exact restoration remain required. A test-runner-local NSWorkspace getter is
not used as an additional system-wide oracle; process-local observations may
differ. Missing, contradictory or
unrestored native Settings records fail receipt admission.
The macOS26 Intel `check-macos-picker` consumer separately
executes `testOwnedNativePickerCompatibility`, the dedicated `picker` case. It
shares the full workflow's original About/license/reopen/Open With prelude and
actual message/folder/keyboard picker interactions, then verifies independent
golden bytes, settled worker presentation and normal hidden-window Quit. It emits
`picker_compatibility` only after the exact scenario-bound preference comparison
and normal owner/runner/resource settlement. Its own XCTest attachment identity
cannot be supplied by the full `workflow` case. Full Services, Finder reveal/
selection/movement/drag, queue/export/destination and every other broader case
remain required on both macOS15 consumers against the same archive bytes.
Both stages enforce their actual OS major and native CPU/target. All consumer receipts bind
the same source/target/archive snapshot and actual host. CI and release
preparation require these distinct stages; candidate success cannot promote
itself to desktop qualification.

Intel26 picker admission uses one `app_picker` job on a fresh hosted VM. Its
`picker_host` policy comes from `xtask/distribution.toml`; unknown image/OS/build
coordinates fail before any service action. `prepare-macos-picker-host` admits
only the exact CI/owner-release workflow file and job, actual native Intel26,
same source/run/attempt/archive/reference, and a non-root current console user
with a graphical Security session. It checks the fixed Apple GUI-user
`com.apple.iconservices.iconservicesagent` identity and bounded native crash
reports. Every selected report must have a recognized schema; at least one must
identify this user's current boot/build, the Apple executable, SIGABRT and the
known Metal/RenderBox fault. See the [runner-image issue](https://github.com/actions/runner-images/issues/14751).

Only after these guards does preparation boot out and disable that exact
GUI-user agent. Console-user identity is rechecked immediately before each
mutation. A nonzero ordinary bootout status is admitted only with immediate,
exact service-absence readback; timeout, signal or uncertain cleanup fails.
Disable must succeed, and readback must show both the absent service and its
explicit disabled override. Environment-bearing launchctl output stays in
private temporary storage; retained observations contain selected fields and
command statuses. Unknown, partial or malformed evidence cannot qualify.

The same frozen app must then pass the full dedicated picker case, including
preferences, original/golden bytes, normal Quit and complete settlement.
`verify-macos-picker-host` performs read-only checks afterward. The final
`picker_qualification` envelope has `mode: modified_host` and retains
`preparation`, `product` and `postverification`. Policy, source/bytes, host boot,
console user and opaque Security-session identity must agree before and after;
Security-session identifiers are not equated with POSIX session IDs. The product
must observe the same OS/build and CPU. Its receipt has no boot UUID: same-job
before/after observations bracket execution rather than directly proving its
boot identity. This qualifies the actual product on the configured modified
host; it establishes neither general unmodified-host compatibility nor that the
service action caused recovery. Earlier failed runs retain their failed status.

The shared [.github/scripts/picker-evidence.jq](../.github/scripts/picker-evidence.jq)
predicates also govern release, rejecting missing/mixed source, policy, workflow,
run, attempt, reference, bytes, method, host, preference or settlement evidence.
The workflow file SHA is recorded separately from tested source S, as required
for owner release dispatch W. [GitHub's reusable-workflow context](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#github-context)
belongs to the caller. No default GitHub context is overridden. All six native
rows and both full macOS15 desktop suites remain required. The separate manual
native-panel diagnostic retains its probe and controlled experiments; probe
success cannot substitute for actual-product qualification.

`cargo xtask distribution` prints the native target policy and expected archive
identities derived from `xtask/distribution.toml` and actual locked Cargo metadata.
It creates no product archive or qualification record. The consolidated CI
consumes its six native build/test rows and separate macOS floor-smoke rows. This metadata task does not initialize the
Unix campaign controller or signal handlers. Its fixed read-only Cargo metadata
subprocess has ordinary Cargo lifecycle behavior, without a campaign cleanup or
execution-deadline guarantee. Windows uses the workflow's explicit native GNU
LLVM toolchain/target invocation; native execution remains separately evidenced.
Compiler pins stay in rust-toolchain.toml and xtask/tools.toml; deployment/runtime
floors in the distribution table are policies, not proof of OS execution.

From a committed clean checkout, install the exact tools selected by
xtask/tools.toml on PATH, then run:

```sh
cargo fetch --locked --manifest-path fuzz/Cargo.toml
cargo xtask fuzz --seconds 60
cargo xtask mutants --initial
cargo xtask mutants --diff origin/main
```

Campaigns clone their exact clean source into owned target/fuzz or target/mutations
storage, retain diagnostics/results, and use an owned Unix process group for
bounded cleanup. Dirty or untracked workspace inputs fail with a commit requirement;
they are never silently omitted or called an empty diff. These bounded local
campaign commands require Unix. This is a tooling condition, not a restriction
on the six native product CLI targets. No full mutation campaign is provided.

Fuzz uses the pinned nightly and cargo-fuzz, all twelve synthetic inputs plus
structural seeds, a fresh writable first corpus, 4 MiB maximum input, five-second
case timeout and 1,024 MiB RSS limit. Runtime independent source authorization and
byte/structure checks run within every preparation. The harness rejects Internal
refusals, checks source subsequence/metadata bounds and independently invokes a
second preparation for fixed-point checks. Crash/timeout/RSS/abnormal exit,
artifacts, missing/zero final execution summary or changed admitted inputs fail.
The harness sets -print_funcs=0 to omit newly covered function-name progress
printing. In pinned libfuzzer-sys0.4.13 this skips only the PrintPC diagnostic loop;
coverage accounting, corpus feedback, sanitizer failure diagnostics and final
execution statistics remain enabled. Unknown or unexpected children fail;
there is no symbolizer-name exemption.

No finite campaign establishes correctness for all byte sequences.

## Pinned cargo-fuzz build limitation

The pinned cargo-fuzz cannot forward --locked to its internal Cargo build. The
driver enforces this bounded alternative:

1. Resolve full metadata with the pinned nightly, --locked --offline and explicit
   host target; capture the admitted lock, manifests, configuration and graph.
2. Build offline using pinned cargo-fuzz and its normal address-sanitized mode.
   Do not add --build-std or copy its instrumentation flags into another build.
3. Before launching any harness, require unchanged inputs and the same full locked
   metadata. Drift fails visibly and is not restored or concealed.
4. Execute the resulting harness directly, with the required bounded arguments,
   owned corpus/artifact directories and cargo-fuzz's detect_odr_violation=0 default.
   Do not cargo-fuzz run after this admission, because it rebuilds.

The internal build does not receive --locked. Pre-execution checks reject graph
drift before fuzzing, but do not retroactively prevent build scripts executing
if unexpected resolution drift occurred during that build. All other repository
Cargo builds retain --locked. Byte identity is an input-control observation,
not proof of fuzz execution or review; the actual execution summary is separate.
See [pinned cargo-fuzz source](https://github.com/rust-fuzz/cargo-fuzz/blob/984c861c8dfea28055254c5f1d2659ab2cd63f76/src/project.rs).

## Scoped mutation selection

Only eml-core authorize_deletions and check_edit_ranges are selected. Discovery
must be positive in both, with at most 400 total. Initial execution covers the
entire selection. Diff mode inspects source hunks against the merge base and
selects whole changed guards, not a first-N subset; unchanged guard selection is
explicitly not applicable and does not cover other changed parser/workflow code.

The driver uses profile mutants, baseline run, one job, copy-target=false,
test-workspace=false, cap-lints=true, --locked/--lib Cargo arguments and15-second
Core library test commands,
120-second per-mutant builds and a 45-minute execution deadline. Baseline
build time is not capped by cargo-mutants' per-mutant option; it is bounded by
that outer execution deadline. Additional bounded cleanup grace is recorded
separately. It checks baseline, each admitted
mutant identity, execution completeness, per-outcome classifications and counts.
Missed/timeouts/incomplete/unclassified results fail; unviable is not caught and
requires retained compiler evidence/source-local review. No global digest-based
survivor registry or automatic equivalence waiver is used.

The single reviewed compiler-invalid structural case is source-local: eml-core
verify.rs authorize_deletions FnValue `Ok(Default::default())`, with its expected
authorization Result type, Build-only failure101 and actual local outcome log
showing only the specific Authorization:Default E0277 cause. Other mutations,
missing/ambiguous logs, another compiler cause, a Test phase, or more than one
unviable result fail. Reports retain separate caught/unviable/reviewed_unviable
counts; reviewed_unviable is never counted as a test catch.

## Verification process lifetime

Private helpers in the developer xtask establish a fresh session and acknowledge
readiness before a worker starts. A root group pin is created and acknowledged
before the worker joins it. Additional process groups are discovered numerically,
rechecked through kernel session/group queries, and joined by owned pins; the
kernel refuses a cross-session join. No guessed group or process-name match
is signal authority. Pins remain unreaped through the last signal and post-stop
observations, even when an original group leader has exited.

Parent INT/TERM/HUP adapters only set cancellation state. Normal control code
closes the parent-only control writer; parent death also produces EOF. Workers
receive null stdin. Supervisor-owned pin writers are not inherited by workers or
other helpers. Pins validate their private session/group/nonce handshake, survive
HUP/INT/TERM and use EOF to terminate their own pinned group. Stopped pins rely on
the demonstrated orphaned-group HUP/CONT cascade under the reviewed group-parent
topology; EOF does not itself wake SIGSTOP.

Cleanup freezes the retained root first, then discovers/pins/freezes additional
groups, requires stable stopped/dead observations, escalates and observes no live
nonadministrative members. Failed/malformed inspection, unstable quiescence,
unsupported guardian-group membership, missing result, unexpected survivors or
unconfirmed cleanup cannot qualify. Worker status, deadline/cancellation and
cleanup evidence are separate; supervisor exit0 is insufficient. Observed elapsed
time is checked even when the worker has already exited between polls.

This controls the reviewed pinned tool topology, not arbitrary process escape or
an atomic universal process-table snapshot. Compiler-wrapper/target-runner hooks
must be absent or explicitly reviewed; the driver refuses unreviewed effective
hooks and captures configuration inputs. OS hangs or SIGKILL of the supervisor
can leave undiscovered groups unconfirmed; pin EOF covers only already pinned
groups. Missing supervisor completion remains failure. Helpers are developer
verification resources and are not introduced into the product CLI/app runtime.

Before starting a worker, the supervisor inspects only its own
PID to check process-row grammar and visibility. This does not establish support
for full-census options or stable session topology; cleanup still performs the
complete census and refuses missing, malformed or unconfirmed observations.
Admission inspection failures emit a terminal private record with their detail
and observed elapsed time. The parent reaps the supervisor and retains that
record before closing admission; it does not invent worker status or cleanup
success.

The process integration harness isolates its outer scenarios with one test-local
mutex because they share a global process census and finite observation budget.
Each scenario retains its own real process concurrency and all negative controls.
The guard covers setup through final observations and resource cleanup, including
panic unwinding. Poison recovery permits later independent tests to run; it does
not catch a failed test or turn Drop into external-cleanup evidence. Normal Cargo
test invocation and native CI use that same isolation without a separate
test-thread flag. This establishes isolated-scenario execution, not simultaneous
independent-controller or ambient-churn resilience. Production session inspection,
unknown/refusal handling and cleanup budgets are unchanged.

The pinned mutation tool's struct-field visitor bypasses its name filter.
Discovery and execution also select the two guard files; function metadata must
still match the complete requested guard set. Core library tests include both
real guards and their rejection controls. Filesystem-heavy integration suites
remain required in ordinary/native verification.

## Scheduled extended checks

The weekly `Extended verification` workflow checks the current default-branch
source with fresh advisory data across workspace, excluded fuzz/filesystem and
enabled macOS dependency graphs. It derives tool versions from the same source,
runs the independent semantic model with4096 cases and the bounded fuzz harness
for900 seconds. The model honors `PROPTEST_CASES` within256..4096, preserving
the normal minimum and limiting inherited requests. Fuzz input, per-case and RSS
limits remain the harness's reviewed limits. Source identity, property failures,
fuzz summaries/logs and crash artifacts are retained; scheduled results do not
replace exact-head six-platform or GUI qualification. No mutation campaign or
publication is scheduled. Workflow implementation does not establish execution; inspect the
[workflow runs](https://github.com/resoltico/EMLAttachmentRemover/actions/workflows/extended-verification.yml)
for scheduled results.

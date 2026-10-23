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

`cargo xtask check-config` checks Cargo manifest ownership and lint inheritance. `cargo xtask check-shell` runs the real ShellCheck on contained tracked/pending/shebang sources; the selected Xcode supplies Swift formatting and real native compiler checks. Both excluded Rust workspaces receive Clippy checks. Exceptions carry source-local reasons and authored unsafe blocks retain concrete `SAFETY:` arguments. See [checks and evidence](../CONTRIBUTING.md#checks-and-evidence).

Native Swift verification uses the selected Xcode's formatter and actual native compilation; another local Xcode does not establish pinned-toolchain qualification. The desktop source gate also runs the three authored `BorrowedFileUrls` lifetime, Send and Sync compile-fail doctests through Cargo/rustdoc, retaining exact selection counts and source-bound causal diagnostics. It deliberately filters 27 upstream vendor doctests. The same gate's Slint Clippy compilation supplies the positive check of the application's actual borrowed callback and `as_ptr()` use. These checks establish compiler contracts, not GUI delivery.

`cargo xtask native-tests --release --target TARGET --out NEW_DIR` requires the
matching native host before test compilation, compares discovered and executed
test identities/counts, and retains transcripts. macOS release selections include
GUI, worker, Darwin and Core fixture features under the pinned Xcode policy.
Windows selections include filesystem, Job and console fixtures. Debug tests
remain required independently. A passing selection establishes only the tested
profile on that host.

`cargo xtask verify-release-inputs --products DIR --desktop DIR --picker DIR --out NEW_DIR`
admits exact current-run/current-attempt product and consumer records, freezing
archive/reference/receipt bytes. `cargo xtask stage-release-assets --inputs
VERIFIED_INPUTS --out NEW_DIR` assembles those bytes and sorted `SHA256SUMS`.
The exact set comes from the typed build plan; assembly is not behavioral
qualification. The final server checker receives the same verified manifest,
compares actual server bytes, then rechecks any original incident identity and
expiry immediately before the owner-only publication edge. See [release
workflow](release.md).

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
On the first failed normal XCTest execution, the owner may sample only its exact
PID, launch date and bundle path before cleanup. This read-only diagnostic has
a two-second sample request, an eight-second process budget and a 5 MiB retained
output limit. Missing, changed or unsettled identities prevent raw retention;
the original product failure remains terminal and no product retry occurs.

The no-universal policy applies to every shipping CLI, app and worker. Xcode may
use `lipo -extract` only on Apple's stock XCTRunner agent for the current native
CPU. The project test bundle uses native ARCHS and ONLY_ACTIVE_ARCH=YES. No
`lipo -create` or test runner/framework/bundle enters shipping archives.
`SWIFTBUILD_LAUNCH_IN_PROCESS=YES` is scoped to xcodebuild; it does not waive
survivor rejection or change saved Xcode defaults.

Ordinary development and test builds retain line tables/backtraces
(`profile.dev.debug=1`) without full local-variable/type debug records. Shipping
release settings are separate. Fresh per-run test builds disable incremental
compilation; discovery and execution still reuse normal Cargo artifacts. After
confirmed process settlement, their executable identities and transcripts remain
while the private compiler tree is removed. A rebound directory or uncertain
owner prevents deletion. The shared macOS release cache is separate.

Native macOS builds share CPU-specific intermediates under `target/native-cargo/`.
CI cache keys include target, locks/manifests, compiler/tool/SDK policy, Cargo
configuration and complete retained vendor sources. There is no source-commit
suffix or broad fallback. Before saving, Cargo removes every authored workspace
package using its actual workspace metadata. Restored dependency objects cannot
substitute an earlier authored build, even with preserved timestamps. Only
compressed registry archives accompany this cache; Cargo regenerates unpacked
sources. Cached intermediates are unqualified: current products are built, frozen,
signed and subjected to image, archive, extraction, behavior and GUI checks.
Qualification records and prior test results are never restored as passes.

The macOS15 desktop and macOS26 picker consumers restore this same exact-key
cache before compiling their tooling, without saving another cache object. This
transfers about 395 MiB per consumer, including native dependency objects their
bootstrap does not use, in exchange for reusing the compressed crate archives.
Consumer tooling still compiles fresh in its ordinary build directory and checks
the frozen product archives. Cache reuse reduces crate-archive downloads; the
registry index and other network prerequisites can still be required. It neither
retries a failed qualification nor makes a cache hit evidence of product behavior.

The fixed Windows PowerShell script uses an ordinary absolute drive expression;
script admission can reject verbatim expressions under the native working
directory. Before launch, the qualifier checks that this spelling resolves to
the same canonical source file. Binary/output expressions and the native working
directory retain their existing forms, and execution policy remains inherited.

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
required role. Missing app artifacts fail their exact consumers, and every failed required producer check fails the aggregate. Available frozen archives still run independent desktop/picker checks when another producer role fails; an app-result file is availability data, never qualification authority. Workflow wiring alone establishes neither product qualification nor publication behavior.

CI admits the actual current-run product, floor-consumer and picker records through `verify-release-inputs`, then runs `stage-release-assets --inputs` to retain the exact candidate assets and checksums as a review artifact. This job has read-only repository permissions and performs no release, tag, attestation or publication operation. Missing or failed native roles prevent assembly and fail the aggregate.

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
finite controlled processing/export I/O and lifecycle cases, export publication
controls, report-writer backpressure and final appearance checks before emitting
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
The macOS26 Intel `qualify-macos-picker` consumer separately
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

Intel26 picker admission uses one `app_picker` job on a fresh hosted VM.
`qualify-macos-picker` first captures complete native CPU/OS/image/boot,
non-root console/graphical-session caller and sanitized `print-disabled` facts.
The parser requires a complete bounded map; empty or unknown output never means
an absent override. A true pre-disabled override or uncertain baseline refuses.

One bounded independent signed AppKit probe runs on that unmodified host. Healthy
phases lead to exactly one frozen-product picker case, followed by a complete
unchanged caller/boot/no-disabled-override snapshot and an empty action list.
The `unmodified_host` receipt never loads the incident registry, reads crashes or
requires a historical service at a fixed path. A later healthy image, absent
historical service or irrelevant expired/malformed incident file cannot block
that route.

Only the exact known probe failure can lazily load the unique matching incident
from `xtask/host-incidents.toml`, requiring current-boot/build/user native crash
proof, exact Apple executable/SIGABRT/Metal/RenderBox frames and the original
loaded service. It constructs a private non-cloneable/non-deserializable
admission owner. Preparation consumes it once. Each fixed bootout/disable edge
rechecks caller/session, boot, clean source/workflow/run/attempt/archive/reference
and original policy digest. Its monotonic deadline is exactly the earlier of ten
minutes and the remaining wall-clock incident lifetime, including fractional
admission time; a rollback cannot extend it. Each action retains actual wall and
elapsed-monotonic guard observations. A failed later guard retains preceding
partial action facts and permits no further mutation.

Successful preparation requires absent/disabled service readback, then one
healthy rerun of the same frozen independent probe. The actual product case runs
exactly once in the chosen state; every product failure is terminal. The
`incident_modified_host` receipt retains the original failed probe, exact policy
identity/dates, admission/actions, recovered probe, complete product projection
and preserved raw product bytes, plus final caller/boot/service readback. It
establishes modified-host coverage without claiming causation or unmodified-host
compatibility. Disposable VM lifetime ends the intervention; no owner-host restore
or service mutation is attempted.

The shared strict Rust `picker_evidence` consumer runs on Linux and macOS. It
rejects missing/duplicate/unknown/object/nullable/mode fields and mismatched
source/run/attempt/workflow/archive/reference/raw-product/method/count/preference/
owner facts. Original receipt bytes remain bound rather than reserialized as a
substitute identity. After server readback, final admission rechecks exact
incident identity and the current exclusive expiry only for modified receipts;
ordinary receipts never consult it. Read-only weekly `check-host-incidents`
maintenance records expired/malformed/closed/unverified upstream state without
renewing policy or retroactively invalidating a normal pass. Standalone panel
diagnostics perform no intervention or product retry. All declared native CLI
and complete macOS15 consumer roles remain mandatory.

`cargo xtask distribution` prints the native target policy and expected archive
identities derived from `xtask/distribution.toml` and actual locked Cargo metadata.
It also records the held source commit, packaging epoch and provenance; source
archives without Git require explicit declarations, which do not qualify a Git
source. It creates no product archive or qualification record. The consolidated CI
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
bounded cleanup. Fuzz and initial mutation snapshots clone only the selected
commit; diff mutation snapshots retain the history needed for the real merge
base. No alternate object store or hard-linked source snapshot is used. Dirty or untracked workspace inputs fail with a commit requirement;
they are never silently omitted or called an empty diff. These bounded local
campaign commands require Unix. This is a tooling condition, not a restriction
on the six native product CLI targets. No full mutation campaign is provided.

Fuzz uses the pinned nightly and cargo-fuzz, all twelve synthetic inputs plus
structural seeds, a 4 MiB maximum input, five-second case timeout and 1,024 MiB
RSS limit. The admitted relative seed names and exact bytes are recorded. The
pinned libFuzzer reads the seed list with `-seed_inputs=@file` and no positional
corpus directory: adaptive coverage/corpus feedback remains in memory, without
persisting every newly interesting input. Crash and timeout reproducers retain
all admitted input bytes in the separate artifact directory. Runtime independent source authorization and
byte/structure checks run within every preparation. The harness rejects Internal
refusals, checks source subsequence/metadata bounds and independently invokes a
second preparation for fixed-point checks. Crash/timeout/RSS/abnormal exit,
artifacts, missing/zero final execution summary or changed admitted inputs fail.
The harness sets -print_funcs=0 to omit newly covered function-name progress
printing. In pinned libfuzzer-sys0.4.13 this skips only the PrintPC diagnostic loop;
coverage accounting, corpus feedback, sanitizer failure diagnostics and final
execution statistics remain enabled. Unknown or unexpected children fail;
there is no symbolizer-name exemption.

Campaign results retain the actual launched fuzz image and its byte identity,
plus any debug companion. Mutation records label remaining terminal build images
as forensic copies, without claiming baseline/per-mutant binary identity. After
known process settlement, held directory identities guard removal of only that
run's `build` and `tmp` trees, including ordinary failed runs. Logs, source,
seeds, outcomes, diffs and reproducers remain. Uncertain ownership or changed
addresses retain scratch and fail cleanup. The extended workflow uploads the
retained images and cleanup receipts before its runner disappears.

Seed byte identities are recorded; adaptive corpus disk growth is zero. One
failing input can still require the full 4 MiB input limit, so shrinking that
artifact allowance would risk losing the exact reproducer. These are corpus and
input bounds, not a filesystem quota for compiler output or logs. Cleanup records
measure terminal compiler scratch and retained image sizes; they do not establish
peak disk use or a universal compiler-space maximum.

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

A manual dispatch of the Extended verification workflow defaults to a 60-second fuzz smoke and omits mutation execution. Selecting `qualification` runs the required 900-second fuzz campaign and `cargo xtask mutants --initial` in its own bounded Linux job, retaining discovery, per-mutant outcomes and compiler logs. A smoke is development feedback and never closes full-campaign acceptance. The property selection remains 4096 cases: its measured execution is about one second, so reducing it would save little. The mutation control host is Ubuntu 24.04 because the pinned GNU tool requires GLIBC 2.39. Native shipping Linux builds retain their declared Ubuntu 22.04 hosts and GLIBC 2.35 limit.

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
the normal minimum and limiting inherited requests. Both existing property
runners set Proptest's shrink budgets to at most 4096 iterations and 2000 ms,
preserving lower explicit requests. This does not preempt an individual case;
the owned outer process deadline remains separate. Before replay, retained seed files must be regular, canonical and at
most 1 MiB/4096 seeds; excess or malformed evidence is preserved and refused,
never silently truncated. Standard Proptest still appends the failing seed after
shrinking, so this is a replay-admission bound rather than a strict append quota.
The replay count matches the largest generated campaign; the byte allowance
permits about 256 bytes per seed on average, while refusing larger retained files
for explicit review. Fuzz input, per-case and RSS
limits remain the harness's reviewed limits. Source identity, property failures,
fuzz summaries/logs and crash artifacts are retained; scheduled results do not
replace exact-head six-platform or GUI qualification. No mutation campaign or
publication is scheduled. Workflow implementation does not establish execution; inspect the
[workflow runs](https://github.com/resoltico/EMLAttachmentRemover/actions/workflows/extended-verification.yml)
for scheduled results.

## Owned native filesystem profiles

The Linux resource record retains both the statfs family label and the actual
`findmnt` mount type/source/options bound to its fixture paths, so the shared
`ext2/ext3` label does not masquerade as a specific mounted ext-family subtype.

Linux native debug and release test selections enable `eml-core/native-fixtures`
and require positive, unique discovery and passing execution of the
[storage boundary controls](../crates/eml-core/tests/storage_observation.rs).
An empty feature-gated suite cannot close that native obligation. Shipping build
feature selection remains separate.

Each native CLI qualifier runs the frozen shipping bytes through finite owned-volume naming fixtures. Linux uses exclusive loop images for ext2/3/4, ext4 casefold, XFS, XFS ASCII-CI and Btrfs, plus an owned tmpfs mount. It separately tests existing-only behavior on ordinary ext4 and an owned OverlayFS whose backing storage is another exclusive ext4 image. The workflow installs the filesystem utilities and records their package versions. Both Linux CPUs require actual execution; a missing formatter, unavailable kernel feature, mount failure or unsettled cleanup remains an unqualified profile.

macOS uses exclusive APFS and HFS+ images in both observed case modes. Windows uses an exclusive NTFS image and both per-directory case modes with volume-scoped 8.3 creation. The Linux OverlayFS fixture additionally refuses new-name Check/Create on a
separate empty owner-private destination, retaining native before/after identities,
mode and modification/change timestamps. This refusal control is independent of
the permissive directories used to challenge existing-only capabilities.
These fixtures never change an existing volume's settings. NTFS evidence records long/short path identity for every generated entry and actual case flags for every generated directory. The configured destination mode must remain stable; descendant modes are observed rather than assumed to inherit. Every naming profile checks independent complete-route tokens and bytes, native source/destination expressions, ordinary entry identities, permission/ownership inventories, Check, Create, reversed repeat and a fresh reversed layout. Actual normalization and mirror coalescing are observed through identities, without a second normalization policy. Missing profiles and cleanup uncertainty fail completeness; a workspace filesystem or another CPU cannot substitute.

Windows namespace snapshots query identity, attributes and last-write time from the same retained native read-attributes handle. [NTFS enumeration metadata can be stale](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilew); a mandatory owned-directory control deliberately changes a timestamp and proves the native snapshot detects the change while the enumerated object remains cached. Whole-namespace metadata equality remains exact.

Windows requires elevated PowerShell 7.4 or later and a real Python installation. Its native command boundary writes raw stdout/stderr through literal file streams, drains both pipes concurrently, and retains the exact child through exit or cleanup; the enclosing private Job enforces the campaign deadline. Each invocation retains start, exit, settlement and bounded error-location facts, including failures before a profile executes. The portable boundary regression is `pwsh -NoProfile -File tools/test_qualify_windows_name_profiles.ps1 -Python <python-path> -Out <new-evidence-directory>`; it does not qualify NTFS.

The `name-profiles` records and raw transcripts are retained with the native candidate. Product failure and unavailable prerequisites remain failures. Independently runnable resource and app work continues when the process controller still admits work; the qualifier preserves every failed required gate and returns failure.

## Controlled preparation resources

The native Linux x86_64 qualifier measures the exact frozen shipping CLI with
five interleaved 32 MiB shallow/deep runs and one exact-total 256 MiB/63-wrapper
run. Independently generated byte-identical existing targets supply a full-byte
candidate oracle; the read-only operation performs no publication or sync, so
those costs are outside this MIME comparison. Deep median must be at most three
times shallow median plus 0.2 seconds; maximum-size RSS must be at most twice raw
bytes plus 96 MiB. Raw GNU-time wall/CPU/RSS, source/binary/fixture hashes,
CPU/OS/filesystem/case mode and complete protocol records are retained under
`preparation-resources`. The enclosing xtask controller owns process-group
settlement and the deadline. These finite measurements supplement the private
actual-work/cancellation tests; v6 observations remain historical baseline only.

# Verification in progress

## Native picker preference observations

The owner-authorized QA scope distinguishes native picker actions from
argument-only testing. Full before/after property lists are retained, alongside
typed JSON preserving Data, fractional Date values, numeric types/precision and
presence. The destination key and every other preference must remain unchanged.
Whole-domain equality is still required for nonpicker/argument-only scenarios.

Only a successfully executed scenario with independent native message/folder
picker observations may change these exact Cocoa state keys:

| Key | Required native type |
|---|---|
| NSNavPanelExpandedSizeForOpenMode | String |
| NSOSPLastRootDirectory | Data |
| NSWindow Frame GoToSheet | String |

This narrow policy follows actual H8dad before/after observations, real picker
interaction and the product's sole explicit setter for copyDestinationBytes;
attributing those changes to Cocoa is a bounded inference, not proof from an
NS-prefixed name. No other OS key is added automatically. Wrong native types,
unexpected changes, destination changes/type changes and these same changes in
a nonpicker scenario fail comparison. Results state whole_domain_unchanged
truthfully, list permitted changes, and separately report unchanged application
and other preferences. No preference deletion/reset/synchronization, retired-key
migration or owner-desktop operation is used to manufacture equality. Product
code neither reads nor writes these OS state keys.

The H8dad GUI XCTest passed but its overall app job failed when the earlier
plutil JSON conversion could not represent the Cocoa bookmark Data. That failure
is retained. Repaired inspection/comparison requires new native execution.

Hosted native app readback now includes an XCTest interaction harness for the
actual extracted app, with an NSWorkspace-returned owned instance and independent
two-file golden checks. At source 8e9e2af, the actual ARM run reached two created
copies but failed the typed XCTest TextView lookup although the AX hierarchy
contained the report. The harness now polls all AX descendants by the observed
report title and still requires the actual settled/reusable report value. At
PR merge 9fdddcd, native ARM run39118566364 passed the complete XCTest case,
independent golden/original checks and Quit cancel/confirm with zero survivors.
Ready/Finished screenshots were exported and the final two-created frame was
visually inspected. The harness also
exercises Quit cancellation/confirmation and compares
the complete app persistent domain using separate fresh observer processes before
and after the argument-domain-only run. It does not run on an owner desktop.
Both independent persistent-domain observations were empty at that ARM merge
checkpoint. This is one real scenario on one CPU, not full GUI qualification.
The Intel run at the same source timed out in the initial AX window query;
cleanup completed with zero survivors. Before making that query the harness now
waits for the owned NSRunningApplication's native finished-launching state and
activates its exact app. Later native passes are recorded below; this alone did
not establish the cause of the earlier Intel AX failure.

H876c480 again timed out on the Intel AX snapshot after native launch/activation;
the owned process group settled with zero survivors. An Intel sampler first
failed writing into the fixture directory, then ran from runner-owned temporary
storage at H515d61b but exited255 without a stack. Its diagnostic output is
retained as an XCTest attachment. The real GUI assertions passed on both CPUs
and both floor hosts; the unsuccessful sampler is removed for the next normal
harness run. Earlier AX failures remain recorded without a claimed proven cause.
The ARM H876c480 failure's retained native AX snapshot identifies both item
refusals as SourceChanged at publication, not a missing UI report. The next
owned harness disables NSWorkspace's optional Recent Items updates for both
launch and document delivery, records native source ctime/mtime/owner/flags and
xattr names, and checks original bytes in teardown even after failed assertions.
Possible LaunchServices metadata interference remains a hypothesis; all Core
identity/digest/signature guards are retained. H eb3addc's ARM app case passed
while its merge 3e32026c ARM case failed; that variation remains explicit.
The local SDK27 Intel source-build probe was run on ARM and caused Xcode to
extract Apple's Intel stock test runner. That probe is outside the native-only
test-infrastructure allowance and is excluded from acceptance evidence. Modified
Intel test code must compile and execute on the real pinned Intel runner.

Each minimum-OS app row waits for the producers but attempts its own exact
current-source artifact lookup even when the other CPU's producer failed. A
missing producer artifact fails that row; it cannot select an older archive.
This lets independent floor checks run on an available qualified producer while
the strict aggregate continues to reject every failed required producer or row.

The no-universal/no-lipo product policy remains absolute for every shipping CLI,
app and worker. A reviewed test-infrastructure clarification permits Xcode itself
to run only `lipo -extract` on Apple's supplied stock XCTRunner test agent for the
current native CPU. Our test bundle is compiled with native ARCHS and
ONLY_ACTIVE_ARCH=YES; no `lipo -create`, product transformation, or test
runner/framework/bundle inclusion in shipping archives is permitted. Local SDK27
test compilation is supplementary, not pinned-Xcode/native GUI evidence.

The XCTest xcodebuild command sets SWIFTBUILD_LAUNCH_IN_PROCESS=YES for that
process only. Swift Build's
[documented default connection mode](https://github.com/swiftlang/swift-build/blob/main/Sources/SwiftBuild/SWBBuildServiceConnection.swift)
honors this switch. This avoids an observed persistent SWBBuildService child;
it does not waive generic survivor rejection or change saved Xcode defaults.
The actual ARM run at 8e9e2af used this mode and recorded zero unexpected
survivors with complete cleanup; its report assertion still failed separately.

Native macOS CLI/desktop compilation shares a CPU-specific Cargo directory under
target/native-cargo/. CI may restore registry inputs and intermediates keyed by
target, lock/manifests, compiler/tool/SDK policy and Cargo configuration. These
are unqualified build intermediates. Cargo still selects the current source and
features; products are separately copied/frozen/signed/staged, then undergo the
same image, archive, extraction, behavior and GUI checks. No archive, signature,
qualification record or prior test result is restored as current evidence.

Windows workspace execution uses the same bounded native-test controller as
Unix, including complete discovery/execution comparison and retained separate
stdout/stderr. Its Windows feature selection includes the real filesystem,
Job and CLI console fixtures. The independent required-name/ignored-name
discovery remains mandatory before execution. This gives a hung test an owned
Job deadline and preserves its transcript before the overall hosted job expires.
M3e32026c's Intel singleton cleanup and app-startup observers failed when external
ps inspection exhausted its two-second budget. The process cases already hold
their own serialization mutex; a redundant global serial-test setting was
removed. The census now asks for the short kernel command name (ucomm), avoiding
full executable-path resolution solely for diagnostics. Numeric PID/group/session
validation, all deadlines and unknown-settlement rejection remain unchanged.
Native Intel inspection and GUI gates remain open until corrected execution.

The first Windows console fixture no longer peeks at the synchronous read
handle shared with the CLI. Microsoft documents that
[PeekNamedPipe can block on synchronous handles with concurrent I/O](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe).
Before launch the fixture queries the actual finite inbound buffer size, closes
its read-end copy after launch, and requires completion of a bounded write
larger than that capacity while retaining the writer without EOF. The real CLI
must consume input for this handshake to finish. This change addresses a
possible fixture deadlock; native execution must still prove interruption.

At M3e32026c the ARM native request handshake completes, but both console cases
fail because the sender exits with 0xC000013A. The sender now retains its handler
until normal exit instead of resetting it through FreeConsole. That actual
corrected console execution is still pending. The same native suite passes the
directory-alias case and the stale final-link rejection control. Its obsolete
tar-extraction Unsupported expectation failed after protected Windows extraction
became available; the shared positive byte/readback and negative material cases
now run on Windows as well as Unix, with platform-specific mode checks retained.

The actual ARM final CLI link succeeds and retains map/verbose/image identities
at M3e32026c. PE policy then rejects bcryptprimitives.dll. It is now explicitly
reviewed as the Windows OS provider for
[ProcessPrng](https://learn.microsoft.com/en-us/windows/win32/seccng/processprng),
which Microsoft's supported-client table places below this product's Windows10
floor and the pinned getrandom source directly imports. This precise DLL
admission does not approve other imports or complete static native-input review.

At H515d61b the explicit-argument capture fails because rustc supplies a response
file rather than positional object paths. That observer was removed. The wrapper
now requests LLD's native
[link reproducer](https://lld.llvm.org/windows_support.html), allowing the actual
linker to expand its own response-file syntax and retain its input bytes before
rustc removes transient objects. A separate read-only bounded tar inspector
records member identities without extracting or executing the reproducer.
Duplicate/traversal/link/oversize controls and a known-payload digest case pass
locally. The input manifest and whole reproducer are bound to the same image/map/
trace receipt. Member identities do not establish source/legal review or which
archive members were selected; native execution and complete static-input review
remain required. Shipping archive readers and their narrow formats are unchanged.

At H66d8acf both Windows native selections pass371 cases. The actual COFF
reproducer is produced, but its reader falsely requires version.txt. The
[pinned compiler LLVM23.1.1 COFF driver](https://github.com/llvm/llvm-project/blob/llvmorg-23.1.1/lld/COFF/Driver.cpp#L2458)
writes response.txt without that version file. The reader now follows that
actual format; the selected linker is separately executed with --version and
checked against the observed compiler's LLVM version, then recorded in the
bound receipt. The wrong-version rejection remains explicit. Corrected native
reproducer execution and static-input review are pending.

Both H66d8acf app producers and both macOS15 same-archive rows pass the normal
basic GUI case with the sampler removed. The extended case for About, complete
license text, keyboard Edit commands and main close/reopen compiles locally but
is not yet executed natively. Review also found that Quit could leave its
confirmation on a hidden main window. All Quit routes now use the shared
show/deminiaturize/front operation before confirmation; duplicate Quit requests
after confirmed shutdown are ignored. The extended test includes native Quit
from a hidden main window, cancellation and final confirmation. Production
stop/force/cleanup timing and ownership are unchanged; native verification is
still required.

At H0f66f2d both Windows native rows pass, including actual reproducer capture
and compiler/linker LLVM-version agreement. Input manifests contain82 x64 and81
ARM files. Selected static-input/source/legal classification and its real
unknown-input control remain separate unfinished gates. The ARM extended GUI
case passes About/version, complete license text, keyboard Edit and close/reopen,
then fails hidden-window Quit restoration. Native intent callbacks and committed
ingress now explicitly post a Slint backend wake after storing their bounded
state; they no longer rely solely on a polling timer with all windows hidden.
The same failing hidden-Quit stimulus/assertions are retained for the next native
run. This is a pending fix, not a passing GUI gate.

`cargo xtask windows-runtime-references --archives DIR --out NEW_FILE` derives
component-inputs.json from the four archives pinned by legal/windows-runtime/
source.json. It checks the Rust authority and archive checksums, retains immutable
owned snapshots, inspects names and streams selected members through the system
tar into fixed owned stdout files, never extracting to advertised paths. It is a
Unix tooling task, not Windows qualification or an approval generator. The
committed references contain42 files per CPU. All31 actual sysroot input files
in each H0f66f2d native manifest match those independent official references.
A real corrupt-archive control rejects at the checksum check and creates no
reference output. Source review, selected-member admission and Cargo-object
provenance remain separate unfinished work.

The next Windows capture checks every observed sysroot file against those
independent references, requires crt2.o and mainCRTStartup, and rejects selected
native support members without a source binding. Bindings cover the32 mapped
MinGW sources (read in full) and the selected LLVM unwind source license
declarations, using the retained complete component notices. Cargo runtime rlibs
must match actual target compiler-artifact paths/bytes; application and symbol
objects must occupy their compiler output roles. Other inputs and build-script
native dependencies are rejected. A native probe is compiled and actually linked
into a second CLI; its rejection must identify that object after successful
linking and actual reproducer consumption. Three local reference/member/path
controls and both Windows-target source Clippy checks pass. Actual execution of
this complete admission/control and archive-bound packaging remain pending.

The hidden-Quit probe is also corrected to use the actual application Quit menu,
with an assertion that opening that menu did not reopen the hidden window first.
NSRunningApplication.terminate returned false without establishing delivery;
the same failed run's genuinely synthesized Command-Q in teardown did expose
confirmation and settle the owned instance. Prior failures stay visible. This
changes the stimulus to the ordinary GUI contract, retaining all visibility,
cancel, confirmation and settlement assertions.

Both H515d61b Windows workspace selections now pass complete accounting:369 cases
each, with the nested cwd child isolated from the outer transcript. Both native
macOS app producers and both same-archive macOS15 rows pass the basic GUI case.
The separate Windows link-observation step remains failed at that H due to the
response-file capture issue above. Those outcomes do not qualify complete v6.

At Hc83b04b both native Windows CPUs execute the real console cases successfully,
and the actual final links pass PE import inspection (ARM119/x64118 imported
functions; no delayed imports). The native workspace aggregate still fails:
the exact-one-test cwd child inherited stdout and leaked a filtered libtest
summary/interleaved case name into the outer unfiltered transcript. Its output
is now captured separately, and the parent requires successful status, the exact
one-case pass and one successful child summary. The outer discovery/case/suite/
filtered checks are retained unchanged. Corrected aggregate execution passes at
H515d61b as recorded above.
At the same H, the ARM app producer and its same-archive macOS15 row pass the
actual basic GUI scenario. Intel app failure and full GUI conditions remain open.
That H's ARM process suite failed its parent-SIGKILL absence probe after two
seconds with the worker stopped. The controller's existing cleanup contract is
ten seconds; the probe now derives its bound from that same constant, checks the
observation deadline before accepting absence/zombie state, and retains the
no-marker-write assertion. The controller's grace, inspection deadlines and
cleanup algorithm are unchanged. This corrects the test's stronger incidental
timeout; actual native parent-death execution is still required.

`cargo xtask release-check --source FULL_SHA --version VERSION` is read-only
admission for a later explicit owner release: exact clean source, fresh observed
main ancestry, Cargo version, one nonempty calendar-dated release section, absent
remote tag, and an absent release or matching unpublished draft. Authentication,
transport and other API errors never establish absence. It derives exactly eight
expected archive names, but does not qualify existing archives, attest, tag,
upload, publish, merge or change settings. v6 remains under Unreleased, so its
dated-release gate intentionally cannot pass now. The publication workflow and
complete release qualification remain unfinished.

`cargo xtask stage-release-assets --archives DIR --out NEW_DIR` assembles the
exact eight archive names derived from Cargo/distribution policy and a sorted
`SHA256SUMS`. It refuses missing/extra/nonordinary inputs and an existing output
address. One bounded immutable input snapshot supplies each copied file and
digest; readback checks the copy. The nine assets occupy a new private enclosure,
and its path/identities are recorded in `NEW_DIR/result.json` outside that asset
set. `assembly_source` identifies the assembling checkout, not archive build
provenance. This command does not inspect package contents, establish native
execution, attest or publish. A future release workflow must first require all
same-source native qualification and bind downloaded assets to those results;
assembly alone cannot admit a release. Its real-file unit cases use synthetic
byte inputs and make no package-integration claim.

The v6 workspace and product remain incomplete. `cargo xtask check` executes
independent Rust, dependency and workflow sections and retains every
failed section. Repository tools are exact-version local prerequisites; CI installs checksum-verified
prebuilt Cargo tools with source-build fallback disabled. Missing
required local Xcode currently prevents a full pass. `qualify`, `verify-archive`, `build-cli-archive` and
`build-macos-sources` distinguish bounded native/archive checks from complete platform
qualification. The full app/archive/GUI and Windows gates have not passed. See
the [implementation evidence](v6-implementation-evidence.md) for exact source
identities and outcomes. The bounded campaign commands are below.

The reusable native workflow explicitly checks out its source input: PR head H,
main source S, or merge-queue source Q. PR integration separately checks out M,
verifies its base/H parents, executes the real Linux workspace selection and
records all three identities. After both branches finish, an API read rejects a
changed PR head/base. The Rust `ci-result` admission rejects absent results,
failure, cancellation and unexpected skips; negative controls exercise each
required role. The delivered workflow version still requires actual GitHub
execution before its source/ref/evidence flow can be called verified.

On Linux and Windows, `qualify` now orchestrates strict release-profile discovery
and execution, the standalone real filesystem probe, native CLI build, complete
materials, final-runtime review, extraction and behavioral smoke. Linux also runs
the real different-UID namespace control; Windows release tests include the
distinct-account, Job and console cases, followed by an actual extra-object
rejection. macOS's `qualify` entry point remains incomplete while full GUI
acceptance is open. Source compilation and scoped checks do not qualify these
new orchestration paths; their native runs remain required.

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
No finite campaign establishes correctness for all byte sequences.

## Pinned cargo-fuzz build limitation

cargo-fuzz 0.13.2 cannot forward --locked to its internal Cargo build. Independent
QA reviewed its exact upstream source and accepted this explicit narrow amendment:

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
test-workspace=false, cap-lints=true, --locked Cargo arguments, 60-second complete Core test commands,
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

## Bounded preparation campaign observations

Source `8e3ed3277dd7faaaee7b665e0901c4f294d38799`, native macOS ARM64:

- Fuzz run `run-aTbUer`: configured 60 seconds, actual 61 seconds,
  1,540,438 executions, peak RSS 552 MiB, normal exit, no artifacts. All twelve
  fixture and ten edge seeds were admitted; locked graphs and inputs were unchanged.
  Controller reports no cancellation/deadline/survivors and completed cleanup.
  Independent QA inspected the raw execution/build/control records and accepted
  this bounded pure-preparation run. It proves no other platform/product gate.
- Mutation run `run-CgbmO2`: baseline build/test passed 59 tests; all fifteen
  discovered mutations were attempted (ten edit-range, five authorization).
  Fourteen individually built and failed guard tests; zero missed or timed out.
  One authorization FnValue replacement, exactly `Ok(Default::default())`, failed
  its Build phase with E0277 because `Authorization: Default` is unsatisfied;
  it had no Test phase. Authorization requires independently derived bytes/proof.
  Independent QA reviewed the producer, diff, full compiler diagnostic and outcome
  accounting and classified this single mutation as compiler-unviable, not caught
  or equivalent. Every viable selected mutation was caught. Original automatic
  result `qualified:false` remains intact; this paragraph records the separate
  reviewed disposition and does not turn an unviable mutation into a test result.

Raw retained evidence lives in the owned clean campaign checkout under
`target/campaign-checkout.EuUzqu/source/target/{fuzz,mutations}/run-*` on this
implementation host. Those files are execution evidence, not reproducible inputs
or a replacement for final-head qualification. No full mutation or 900-second fuzz
campaign was run.

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

The process integration harness isolates its outer scenarios with one test-local
mutex because they share a global process census and finite observation budget.
Each scenario retains its own real process concurrency and all negative controls.
The guard covers setup through final observations and resource cleanup, including
panic unwinding. Poison recovery permits later independent tests to run; it does
not catch a failed test or turn Drop into external-cleanup evidence. Normal Cargo
test invocation and native CI now use that same isolation without a separate
test-thread flag. This establishes isolated-scenario execution, not simultaneous
independent-controller or ambient-churn resilience. Production session inspection,
unknown/refusal handling and cleanup budgets are unchanged.

The current pinned mutation tool's struct-field visitor bypasses its advertised
name filter. Both discovery and execution also select the two real guard files;
function metadata must still match exactly the requested complete guard set.
The first escaped selection was refused before any mutation execution. A later
unmutated baseline exceeded the former 15-second whole-Core command allowance;
no mutant was tested or called caught. The bounded command allowance is now 60
seconds, with the 120-second build and 45-minute campaign bounds retained. Baseline
execution, complete selection, strict result classification and timeout failures
remain required. The subsequent full 15-case run passed 14 caught plus one reviewed
compiler-invalid case, with none missed or timed out. Fuzz at 9ebcdee passed
1,710,574 executions in its 60-second configured campaign; see exact source records
in the implementation evidence before treating an earlier source as current.

# v6 implementation evidence

Status: implementation in progress; no v6 product or archive qualified.

## Fixed scope and baseline

The owner requires Rust 1.99.0 reusable business libraries, a thin Rust CLI,
a fresh SwiftUI app, full Python/product-tooling and old AppKit UI removal,
no compatibility or migration, six native CLI targets and two native macOS apps.
No tag, publication, merge, approval, settings change or pushed-history rewrite
is authorized. v6 release outcomes will remain under Unreleased.

Published baseline, current base and fetched origin/main:
`f7adf489734beff3ca94f829f8db1c110be316f4` (v5.0.0).
`git log BASE..origin/main` was empty after fetch: no later main changes.
The owner's checkout was clean and remains untouched.

## WP0 commands and inspection

- Read governing AGENTS.md, complete PROMPT.md and every design document,
  contract, material and evidence/probe source in README reading order.
- `shasum -a 256 -c eml-attachment-remover-v6-design-2026-10-15.tar.gz.sha256`
  from the archive directory: passed. The first invocation from the repository
  failed to locate the relative archive name; it did not establish a mismatch.
- `tar -tzvf`: 34 regular files, one relative top-level prefix; no links,
  devices, absolute paths or parent-traversal members. Extracted with tar into
  a new empty directory outside the repository.
- Created a separate managed worktree on `codex/v6-rust-swiftui` from origin/main.
- `rustc +1.99.0 -vV`: 1.99.0, b940084d7eb6a299eb4bfeb8e34901bc051e7ac4,
  aarch64-apple-darwin, LLVM 23.1.1. Runtime machine arm64; macOS 27.0.
- Full Xcode selected only through invocation-local DEVELOPER_DIR:
  Xcode 27.0 build 27A266a, Apple Swift 6.4. This is additional local evidence,
  not the required Xcode 26.3/macOS 15 hosted qualification.
- Approximately 148 GiB free at prerequisite inspection.
- Original supplied filesystem probe copied into owned target scratch,
  `cargo +1.99.0 run --locked --offline`: passed on macOS ARM64, including
  mode checks, held candidate identity despite replaced paths, no-replace
  conflict, FIFO and file/directory synchronization. This is primitive evidence,
  not product/native6 qualification.
- Foundation argument-domain preference probe with empty String override:
  passed without saved preference writes.
- Early Windows feasibility source type-checks for x86_64-pc-windows-msvc.
  This is only a compile check, not GNU LLVM/native Windows evidence.

## Early design challenge

contracts/verification.md requires ordinary-user NTFS native feasibility on both
Windows CPUs before substantial frontend work. The early probe tightens the
supplied destination handle to FILE_SHARE_READ only and verifies actual native
machine via IsWow64Process2. It performs the real write, file flush,
held-source/relative-root replace-false rename, conflict, parent rename denial,
readback and directory flush sequence. Failure is retained, not waived.
The initial probe does not yet establish the full ancestor-chain/junction,
ordinary-user token, ACL, long-path or adversarial product requirements.

## WP0 gate checkpoint

This initial pending record is historical; later checkpoints record the bounded
subsets actually executed. No complete v6 product/archive qualification exists.

Pending: WP1–WP8, all product checks, native Windows ordinary-user feasibility,
full six-platform filesystem tests, independent MIME goldens/edge/model tests,
CLI protocol and size bounds, bounded 60-second fuzz, scoped complete mutation
selection and detection demonstrations, exFAT observation, both native GUI
integrations/screenshots/preferences evidence, eight archive inventory/runtime
notices/provenance/extracted execution, final clean-clone checks, exact-H
native6 and current-base exact-M integration. None is inferred from this log.

## Native Windows design challenge results

The first two native runs failed the selected relative-root rename route:
[initial](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38757316025),
[operation-specific diagnostics](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38757524849).
The restricted-token, complete ancestor-pinning run also failed:
[4604070](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38758032693).

[Native comparative run at cd8e09c4bd6df604435fd8b2e2a0493ccf3efced](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38758392136)
executed three real-boundary experiments on each Windows CPU:

| Experiment | Native x64 | Native ARM64 | Qualification |
|---|---|---|---|
| Required relative name + held RootDirectory, no write/delete sharing | error 87 | error 87 | Failed |
| Absolute name + NULL RootDirectory, same protected ancestor chain | error 32 | error 32 | Failed alternative |
| Absolute name with destination write sharing enabled | exit 0 | exit 0 | Unsafe diagnostic only; concurrent directory write open succeeds |

The last experiment demonstrates why allowing write sharing cannot simply turn
this into a supported product. That comparative workflow failed; it did not waive
the required route or junction protection. At cd8e09c no NT operation had been implemented.
The peer QA chat has been asked to review the source, native evidence and
possible safe corrections and reply directly to the implementation chat.

Both jobs observed NTFS through GetVolumeInformationByHandleW, verified actual
native machine through IsWow64Process2, and executed with a restricted effective
token: Administrators deny-only, DISABLE_MAX_PRIVILEGE, membership check false.\nSeChangeNotifyPrivilege remains available for traversal.
This records actual access restrictions; it is not evidence from a separate
ordinary-user interactive login or the full adversarial product test matrix.
Both actual file flush and prepublication directory flush succeeded. The failed
rename must not be misreported as a namespace flush failure.

- x64: windows-2022, Windows Server 2022 Datacenter 10.0.20348,
  runner image 20261004.326.1, native machine 0x8664.
- ARM64: windows-11-arm label, Windows 11 Enterprise 10.0.26200,
  actual image windows-11-vs2026-arm64 20261013.183.4, native machine 0xaa64.
- Both: Rust 1.99.0 b940084d7, native GNU LLVM host, selected sysroot rust-lld,
  explicit target, self-contained linking and +crt-static.
- Generated FILE_RENAME_INFO layout: FileName offset 20, size 24 on both CPUs.
  Initial padded/NUL and later exact-length buffers both failed relative rename.
- Three experiments per CPU, six total in the comparative run. These are probe
  executions, not a count of product unit/integration tests. No native six-target
  artifact or archive was built.

## Other observations

- Repeated the supplied macOS primitive probe under umask 000: passed.
- Both empty and nonempty Base64 String argument-domain preference overrides
  passed the Foundation-only probe without saved preference writes.
- An owned exFAT image creation attempt under target/exfat-feasibility failed:
  hdiutil: create failed - Operation not permitted. No attachment/mount/device
  was created. Product exFAT qualification is unrun; no supported-volume claim.
- Early probe rustfmt, Clippy with warnings denied (including the Windows source
  compile check), actionlint and git diff whitespace check passed locally.
  These checks do not qualify runtime behavior or a v6 product.

At cd8e09c the native-route challenge was unresolved pending peer QA. All WP1–WP8
product acceptance gates remain incomplete; the published v5 product/tooling,
artwork, AGENTS.md and release history are preserved while that prerequisite is
resolved. This branch is not a dual-engine product or a delivered v6 purge.

## Resolved Windows mechanism

The independent QA chat reviewed the proposed same-directory NT rename and
both native executions in run38759803940 at e79277b. Both CPUs passed with
protected sharing retained, NT success/completion 0, no-overwrite conflict
STATUS_OBJECT_NAME_COLLISION 0xC0000035, final identity/bytes and directory
flush. The overall comparative workflow stayed failed because it also ran the
superseded Win32 operations. Those historical failures remain evidence.

The accepted narrow technical correction is recorded in
[windows-publication.md](windows-publication.md). No NtCreateFile or safety
relaxation is added. The active early workflow now requires that corrected
protected operation and its negative controls. Superseded experiment branches
were removed from the active probe after recording their outcomes. The pending
hardening rejects dot/dot-dot/over-limit names, uses conservative native buffer
allocation, and labels immediate-failure IO status slots non-authoritative.
The earlier blanket “privileges disabled” probe wording is corrected: the
restricted runner-account thread token retains SeChangeNotifyPrivilege.

Independent closure QA at cf72dec9404786c31ea9741d0418422203ec7214 confirmed
both protected native jobs succeeded in [run38760820393](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38760820393).
This closes focused early primitive feasibility. Full WP3 remains unqualified.

## Pure MIME implementation checkpoint

The workspace now contains eml-core's immutable physical parser, bounded control
syntax, planner, separate source authorizer/byte reconstruction, candidate
reparse/structure verification and per-run cancellation. No filesystem workflow,
CLI, SwiftUI application or packaging qualification is claimed at this checkpoint.

Independent goldens were authored from explicitly selected source delimiter spans
before using the new processor. Inputs remain byte-identical to the design pack.
Manual diff review of 03/04/05/11 confirms only the authorized removals/collapse.
EML files are binary in Git attributes. All twelve specified outcomes pass.

`cargo +1.99.0 test --workspace --locked`: 3 unit + 20 integration tests passed,
0 ignored, 0 filtered. No doctests are defined at this checkpoint. This is an
initial subset, not completion of the full required MIME edge/model matrix.
An initial LF/CR metadata expectation incorrectly counted five rather than four
entity header terminators; independent source inspection corrected the oracle
from 502 to 503 entity bytes. Exact output bytes already matched the golden.
`cargo +1.99.0 clippy --workspace --all-targets --locked -- -D warnings` and
`cargo +1.99.0 fmt --all -- --check`: passed after resolving the reported lints.
No bounded fuzz/mutation campaign has run yet.

## MIME grammar and adversarial matrix checkpoint

Independent QA reproduced three display/diagnostic defects at 5caf7a6: unquoted
comments were literal parameter data, malformed encoded-word prefixes could
rescan quadratically, adjacent valid words retained LWS and invalid Q words
were decoded, and nested syntax errors lost known source sections. These have
focused source fixes and independent regressions. The encoded-word scanner uses
75-byte lookahead rather than a timing-only test. Current grammar clarification
and deliberately tolerated display syntax are in [mime.md](mime.md).

`cargo +1.99.0 test --workspace --locked`: 57 tests passed (5 unit including
one differential property, 7 display, 12 resource-boundary, 1 model property,
20 preparation/golden, 12 structure), 0 failed/ignored/filtered. The model is
configured for 256 generated trees and the independent mail-parser comparison
for 64 generated CRLF/identity cases, with owned regression seeds retained
under target/proptest. The differential checks exact raw leaf spans/types,
not regenerated/decoded output, and excludes embedded coordinate systems;
embedded messages remain covered by independent model and golden tests.

A model failure found a renderer-oracle mistake: CR-ending empty child plus
LF parent framing combines into one CRLF. The model now isolates framing with
CRLF and retains mixed entity/container transport, and an explicit exact-wire
regression establishes the merged-terminator entity byte count. Another initial
differential assertion confused shallow node allocation order with physical
source order; both independent fact lists are now paired by source offsets.
Neither correction changed the parser to match a mistaken oracle.

Limits tested at/above their specified boundary include 256 MiB raw input,
64 levels, 10,000 entities, per-block/aggregate header bytes, per-entity/total
fields, per-field/total parameter pieces, continuation indices, comment depth,
media tokens and related start identifiers. These passing tests do not replace
the still-required bounded fuzz, scoped mutation and deliberate defect runs.

## Rust campaign tooling checkpoint

Rust xtask now exposes the bounded fuzz and scoped mutation commands with one
version authority at xtask/tools.toml. The separate fuzz workspace/lockfile and
synthetic fixture/structural seeds are committed inputs. No actual campaign is
claimed merely from driver tests or mutation discovery. Discovery at 7d0c65c
found 15 mutants: 10 in check_edit_ranges and 5 in authorize_deletions.
The pinned mutation profile compiled successfully with Rust 1.99.0.

The cargo-fuzz build limitation and pre-execution amendment are documented in
[verification.md](verification.md). Driver negative controls reject input/graph
drift before a real harmless marker process can launch; valid inputs do launch
it. Other controls reject missing/zero fuzz summaries and incomplete, duplicate,
failed-baseline, missed, timed-out or inconsistent mutation outcomes. A real
owned-process deadline test exercises termination/reaping. These tests prove
controller properties, not actual fuzz execution or native product integration.
The remaining check/qualify/archive/notices/release entry points are not yet
implemented or claimed as passing at this checkpoint.

## Verification lifetime correction checkpoint

Independent QA found that the initial developer driver could leave nested
cargo-mutants process groups after parent termination, accept an overdue observed
success and resolve origin/main against the clone's different origin. No named
campaign was run with those known defects.

The replacement uses private developer-only session/pin helpers, parent signal
and EOF control, startup/waitability admission, scoped same-session group joins,
unreaped pin custody, bounded freeze/rescan/KILL observations and separate worker/
deadline/cancellation/survivor/cleanup evidence. Setup capture commands use the
same controller and separate stdout documents from stderr diagnostics. BASE is
resolved to an immutable commit in the caller checkout before clone/fetch.
Compiler/runner/wrapper hooks require explicit review. The actual assumptions
and failure limits are in verification.md; there is no universal process-table
snapshot or malicious-daemon sandbox claim.

Local macOS 27 ARM64: 17 actual process probes passed, 0 failed/ignored/filtered,
including parent INT/TERM/KILL, ordinary-background and nested ignored-TERM
workers, late group creation, cross-owned-session EPERM, unreaped dead pins,
TERM-surviving pins, frozen supervisor death HUP/CONT cascade, startup/inspection
failure, overdue completion and separate JSON/stderr capture. An initial
post-termination observation failed under PID churn; bounded retries now require
two stable clean observations rather than treating instability as success.
A shell SIGCHLD-ignore launch remained waitable on this host; its probe now
requires either actual startup refusal or complete real waits/cleanup, without
claiming unsafe disposition inspection. One accidentally empty exact-name
selection was rejected as evidence and rerun using its full test name (1 passed).
Clippy with warnings denied and actionlint passed. The new process-probes workflow
runs actual Linux/macOS counterparts; their results remain pending until execution.

## Process controller closure deltas

At 57c8e3c, actual process-probes run38787383185 succeeded on Ubuntu22 x64 and
macOS15 ARM64: 17 tests on each, Rust1.99.0, no skips. At 26b88b8 the same
native counterparts succeeded in run38787995601. This is process-boundary
qualification only, not native product/archive qualification.

Subsequent source fixes propagate one absolute cleanup deadline through each
inspection, row/pin startup and ACK wait instead of restarting per-group budgets.
A delayed 300ms pin ACK is rejected under a 20ms remaining budget. Already-reaped
worker PID numbers are not exempted from unexpected-live-survivor accounting.
The supervisor's startup now proves dead-unreaped pin retention using safe
waitid(EXITED|NOWAIT|NOHANG), a second same-group join, last signal and real reaps
before readiness. Effective Cargo admission runs at the snapshot CWD and at
command construction; standard Rustdoc/native compiler override families and
unreviewed build/target flags are covered. A real inherited target/.cargo config
case blocks command construction/marker launch and its clean positive launches.

Local updated checks: 8 driver unit tests and 18 actual process probes passed,
no failures/ignores/filters. Clippy with warnings denied and formatting passed.
The bounded named fuzz/mutation campaigns remain unrun pending independent
controller closure and exact-revision native probe results.

## Native request resource admission checkpoint

`eml-core` now exposes native `RunRequest`, `OutputLocation` and `RunMode` values
and a pure `validate_path_limits` operation. Argument count is checked before
deduplication. The 4 MiB aggregate covers input paths only, counting Windows
UTF-16 units as two bytes each. Output and captured absolute working directory
receive separate per-path checks. Raw dot/dot-dot components count without
normalizing or resolving them; root and Windows prefix do not count. Validation
performs no filesystem access, events or process-directory changes. It is not
complete run admission: destination existence and source binding still need
implementation.

Local macOS ARM checks: `cargo test --locked --package eml-core` passed 71 tests
(8 unit, 8 display, 12 limits, 1 model, 20 preparation, 9 request, 13 structure),
zero failures/ignores/filters. Rustdoc discovered zero doctests; this is not
additional test coverage. `cargo clippy --locked --package eml-core --all-targets
-- -D warnings`, formatting, workflow actionlint and diff whitespace checks passed.
Windows MSVC `cargo check --locked --package eml-core --all-targets --target
x86_64-pc-windows-msvc` passed as compile-only preflight, not native qualification.
The six actual native request-test executions remain pending at this checkpoint.

The independent coordination chat reviewed the resource-admission design and
confirmed input-only aggregate scope and before-deduplication argument counting.
The unrelated Windows creation-protection requirement remains failed on both
native CPUs; [its record](windows-publication.md#native-creation-protection-gate-failed)
identifies the exact run and tests that did not execute. The real failing check
is retained in CI; passing request tests cannot qualify publication.

At exact head `a5acea947e71f61258d78d7a57bcebfdebbb2231`, all six native
request-test executions passed: nine tests each, zero failures/ignores/filters.
[Unix run38813265190](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38813265190)
passed all six jobs, including the existing filesystem and process checks.
[Windows run38813265183](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38813265183)
passed both request steps, then failed the separate creation-protection test on
both CPUs (zero passed, one failed, nine filtered each). Later filesystem library
tests remain unrun in those jobs. These results close only the native pure
resource-admission slice, not WP4 or Windows publication.

## Protocol request decoder checkpoint

The real frontend package now contains strict request decoding as a library;
there is no executable or stand-in processor. Local `cargo test --locked --package
remove-eml-attachments --test request` passed 16 integration tests, zero failures/ignores/filters;
unit and doctest discovery was zero. The required-nullable omission, unknown and
duplicate fields, malformed JSON/UTF-8, canonical Base64, host mismatch, native
units and core path/aggregate limits have independent negative cases. A valid
request padded to exactly 8 MiB passes; one byte over fails before parsing.
Error diagnostics are checked for bounded size and absence of request content.

Independent review found that derived deserializers accepted positional arrays
for required objects and tagged objects for encoding strings. Two regression
tests each failed against the original decoder before correction. A map-only
visitor now guards root/input/output objects, and encoding deserialization accepts
only strings. Both controls and all prior request tests pass after correction;
missing nullable output remains an error. This is actual input-validation evidence,
not a claim about the unimplemented stdin reader or event transport.

Frontend all-targets Clippy with warnings denied passed on the local host and
for Windows MSVC as compile-only preflight. Its native six-platform tests are pending
at this checkpoint. Lockfile inspection found only the new local package entry;
no resolved upstream dependency version changed. The eventual executable still
needs interruptible reading, signals, checked rendering and the core run API.

The broader local `cargo test --locked --workspace` run did not pass: core71,
filesystem25, decoder16 and xtask9 passed, then the process integration suite
reported nine passes and nine failures under default parallel execution. Failure
reports retained under `target/process-tests/` include unsuccessful cleanup
observations. The existing CI invocation,
`cargo test --locked --package xtask --test processes -- --test-threads=1`,
subsequently passed all18 with zero failures/ignores/filters. This serial result
does not close the default-parallel failure or prove cleanup of its earlier
resources. Independent process-test isolation/lifecycle review remains pending;
the decoder slice does not alter that code or remove the failing observation.

At exact commit `4f285c83dd9d8f967576ae87921f26a600e73fb9`, all six native
decoder steps passed 16 tests each, and all six resource-admission steps passed
nine each, with zero failures/ignores/filters. The
[Unix/process run38814349629](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38814349629)
passed all six jobs. The
[Windows run38814349638](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38814349638)
passed both independent decoder/admission steps, then failed the unchanged
creation-protection gate on each CPU (zero passed, one failed, nine filtered).
Later Windows naming/metadata tests did not execute. These results qualify only
the implemented decoder/admission slices; executable lifecycle, product workflow,
archives, GUI and complete platform publication remain incomplete.

## Typed diagnostic and independent Windows check ordering checkpoint

Core now owns 27 semantic codes, 12 phases, their stable spellings and permitted
phase pairs. Exhaustive pure preparation classification preserves its existing
MIME refusal domain; `InvalidRequest` remains pre-stream. Three new tests compare
every pair with the hand-reviewed protocol table and cover preparation and
interruption boundaries. Local core74 tests passed, zero failures/ignores/filters;
core all-targets Clippy passed locally and as Windows MSVC compile-only preflight.
Independent source review reran core74/Clippy and found no concrete defect.
This does not implement the runner or enforce pre-edge interruption by itself.

The Windows workflow now separates existing primitive, naming and read/path tests
from the real required creation gate. Independent steps and the gate run after
successful native-toolchain/scratch setup even if another test step failed;
there is no continue-on-error. The gate's exact discovery, execution and assertions
remain intact. Only that separately executed test is filtered from the other
library run. This ordering is reviewed and actionlint passes; its native outcomes
remain pending until the next exact-head run. The separately reviewed Windows
creation proposal remains an unqualified direction, with no product-code change.

At exact head `5974fecc11a2e896070fd732c65720f3fc462683`, the
[Unix/process run38815521087](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38815521087)
passed all six jobs. Diagnostic tests passed three cases on each of the six
native platforms, with zero failures/ignores/filters. The
[Windows run38815520996](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38815520996)
passed primitive, naming (three passed, five intentionally filtered), and the
previously unrun file metadata/path tests (nine passed, zero failed/ignored,
one filtered for the separately executed gate) on both CPUs. The helper child
reexecutes one existing path case; its pass is not another distinct test.
The four new metadata/read cases cover zero-ID refusal, nonregular-file refusal,
hard-link identity/excluded writer and changed-parent binding. Both real creation
gates then failed with zero passed, one failed, nine filtered. Thus the independent
checks are now observed rather than hidden behind that failure; Windows product
creation is still unqualified and the default-parallel process issue remains open.

## Distribution identity planner checkpoint

`xtask/distribution.toml` is now the distribution policy authority for the exact
six platform/target/runner records and deployment/runtime floors. The implemented
`cargo xtask distribution` command derives native rows, static platform policy
text and eight sorted expected archive identities. Version comes from actual
locked/offline Cargo metadata, with workspace/package membership, source and
manifest checks. Output is bounded metadata, not archives or qualification.
Both current filesystem evidence matrices now consume those rows; the separate
process-probe subset retains its own scope. Native Windows planner execution and
unit tests precede the still-required real creation gate.

Local macOS ARM: all15 xtask main unit tests passed, including six new distribution
tests (zero failures/ignores/filters). The initial six-test selection intentionally
filtered the nine existing tests; the complete15 run did not. Independent review
reran all15 and local all-target Clippy and inspected the real command: version
6.0.0, six rows, eight expected names, 3,555 output bytes and no campaign-helper
creation. Local and Windows MSVC all-target Clippy passed; the latter is compile-only
preflight. Both actual workflow projections produced the required four/two rows
locally. Formatting, actionlint and whitespace checks passed. Native planner and
portable tool-unit execution remain pending until the next exact-head CI run.

Static Unix cfg annotations remove Windows-only unused imports/items/fields in
the controller module without changing Unix forms or behavior. Campaign commands
still initialize the same controller/signals; the metadata task bypasses them.
Neither the unresolved default-parallel failure nor Windows publication is fixed
by this planner. Required check/qualify/archive/notices/release tooling, full native
products, GUI and artifact qualification remain incomplete.

At exact commit `08651c492e1738aa01a43e58b798ad6d0f695bba`, both dynamic
matrix jobs and all six native planner invocations succeeded. Parsing their
actual JSON stdout found version6.0.0, six target rows and eight expected archive
identities in every invocation. The
[Unix run38818132386](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38818132386)
passed all seven jobs, including 15 tool unit tests and 18 serial process tests on
each process host. The
[Windows run38818132412](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38818132412)
passed the metadata job and each native planner/tool-unit step (12 unit tests per
CPU, zero failures/ignores/filters; three Unix-only tests have their actual Unix
counterparts). Independent request9/diagnostic3/decoder16, naming3 and non-gate
filesystem9 checks passed as before. Each actual creation gate still failed
zero passed/one failed/nine filtered. The Windows workflow therefore remains
failed, and the default-parallel process limitation is still unresolved. This
is planner portability/source evidence, not product, archive or app qualification.

## Bound-file observation typing checkpoint

Both native bound-file implementations now return neutral typed observations:
initial `NotRegular`, proved `Changed`, or `ObservationFailed` with the original
I/O/validation cause. Existing-destination use is supported by the same type;
core still owns workflow codes/phases. Failed observations are not inferred to
be changes from prose or I/O kind. Native causes survive the Unix inventory I/O
adapter. Source access/sharing/reparse flags, positional reads, publication, ACLs,
cleanup and process behavior are unchanged.

Local macOS ARM filesystem tests passed27, zero failures/ignores/filters; doctests0.
Local and Windows MSVC all-target Clippy passed, with MSVC remaining compile-only
preflight. Independent review reran27/Clippy and inspected actual caller migration,
positive type/change cases and failed-observation distinctions. Windows parent
handles remain transient, not continuously pinned by this read API. Its UNKNOWN
file-type classifier has logical branch tests and a separate real directory
metadata test; actual native Windows outcomes are pending the next exact-head run.
No core reader/dependency or product publication implementation is claimed here.

At exact head `15c40086c1a18464a407af4f5964667d58edf81e`,
[Unix run38822402186](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38822402186)
passed all seven jobs: Mac filesystem27 and Linux23 on each CPU, zero failures/
ignores/filters. The
[Windows run38822402187](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38822402187)
passed both non-gate filesystem suites13, zero failures/ignores, one separately
executed gate filtered. Native directory/error distinctions and strengthened
existing cases executed on both GNU LLVM CPUs. The child path reexecution is
not another distinct case. Each real creation gate still failed zero passed,
one failed,13 filtered; only those two gate steps failed. Request9, diagnostic3,
decoder16, planner/tool12 and naming3 remained successful. This closes native
fact typing only; no Windows publication or default-parallel process fix follows.

## Held-source preparation capability checkpoint

Core now combines its private bounded reader with the existing pure MIME preparer
through `prepare_source`. Private-construction `PreparedSource` owns the original
bound file and immutable `PreparedMessage`, without duplicate inventory signature
or raw-input retention. Source length derives from the validated initial signature;
phase-free revalidation remains available for the later run workflow. Data-read
I/O and metadata/binding errors retain distinct causes. No output or receipt/run
model is implemented by this capability.

Local full core suite:87 passed (15 unit,3 diagnostic,8 display,12 limits,1 model,
20 preparation,9 request,6 native source,13 structure), zero failures/ignores/filters;
doctests0. Targeted private reader selection passed7 with eight other unit cases
intentionally filtered. Real file tests include independent golden bytes/source
preservation, held lifetime, stale inventory, stop/refusal, later observed changes
and actual256 MiB/one-byte-over reading. Focused loop cases cover short reads,
one-byte EOF, cap/initial-size distinction, cancellation/EINTR and cause context.
Core all-target Clippy passed locally and as Windows MSVC compile-only preflight;
an initial unused-unit-expression lint was corrected without suppression.

Root lock changes add only core's filesystem/tempfile edges. The separate fuzz
lock adds the seven required existing-root-version filesystem graph packages;
no previously locked version changed. Locked/offline complete fuzz metadata
resolves. This is graph verification, not a new fuzz campaign. Native six-platform
source-capability execution remains pending the next exact-head run. Full run,
CLI executable, publication, packaging, GUI and required unrun gates remain open.

At exact head `c2964838fec8636ce289789e32541c3a7a35acad`,
[Unix run38826978536](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38826978536)
passed all seven jobs. Each of four native Unix targets executed core unit15 and
source6 with zero failures/ignores/filters. The
[Windows run38826978540](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38826978540)
executed unit15 and source5 on each native CPU with the same zero counts. Windows
has a separate actual held-read-sharing assertion; the later-change test is Unix
only. Both actual256MiB/one-byte-over source cases ran on all six. Other existing
native suites remained successful. Each Windows creation-protection gate still
failed one case with13 other library cases filtered; only those gate steps failed.
This closes native execution of the held-source capability, not complete WP2/WP3.

## Windows selected-directory names checkpoint

The read-only capture/enumeration implementation has eight focused cases: three
pure native-record parser controls, actual multi-buffer names/native units with
partial/full restart, actual capture/open failures, a synthetic malformed-buffer
terminal-error control, actual non-directory query/terminal-error/restart, and
explicit selected-directory-link target identity. Multi-buffer payload alone
exceeds64KiB. Selected-root link and large listing cases use the existing verified
restricted runner token; no privileges are enabled. Native query failure uses a
real owned regular-file handle, violating directory admission only inside the
private test. It is separate from public capture and synthetic parser evidence.
Skeptical review corrected an initial test premise: absence of list access is
not a documented failure guarantee for this query class. The replacement checks
a real non-EOF error without assuming a particular native error code.

Local Windows MSVC all-target compile/Clippy preflight and unchanged macOS
filesystem27 passed. Initial new-code Clippy findings (fixed-size chunks and an
iterator loop) were corrected without suppression. No Windows execution of this
new slice is claimed until the next exact-head native run. Existing creation
protection remains failed and actually enabled. No writable Windows backend,
recursive collector, archive or GUI gate is closed by names enumeration.

At exact head `a14f0e50641e745462e8dd9ca46575a0fede28d0`,
[Windows run38833711801](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38833711801)
passed all eight inventory cases individually on both native GNU LLVM CPUs.
The non-gate library suite passed21, failed/ignored0, filtered1; its child-path
reexecution is a repeat, not another distinct test. The deliberately invalid
regular-file directory query returned native5 on each CPU, observed rather than
assumed by the test. The actual creation gate still failed0passed/1failed/
0ignored/21filtered on both; only those two gate steps failed. Request9,
diagnostic3, decoder16, unit15/source5, tools12, planner and naming remained
successful. [Unix run38833711835](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38833711835)
passed all seven jobs, including filesystem27 on both Macs/23 on both Linux CPUs,
real other-UID namespace checks and the two serial process suites18. All four
Unix unit15/source6 sets passed without filters/ignores. Logs are retained in
owned target scratch. This proves the tested names-only slice, not Windows
root-chain/child-descent admission or publication. Default-parallel process-test
failure recorded earlier remains unresolved by the successful serial suites.

## POSIX selected expressions anchored to a held cwd

The next read-only admission prerequisite adds explicit selected-expression
observation/open relative to an existing cwd descriptor. Common metadata
classification and directory construction are reused. The separate skeptical
pass checked moved cwd addresses, absolute expressions, symlink/dot-dot resolving
to a different physical parent, preserved requested spelling, initial regular/
special metadata, failed empty/missing/NUL expressions and unchanged walked-child
no-follow policy. Three real owned-fixture tests cover those cases, including
native names and an unwritten FIFO. Local macOS filesystem30 and all-target
Clippy passed, zero failures/ignores/filters; doctests0. Two initial fixture
compile attempts used Rustix FIFO functions unavailable on macOS and were
corrected to the project's existing owned `mkfifo` convention. The first full
run then passed29/failed1 when APFS refused invalid-UTF8 fixture creation with
EILSEQ92 before product observation. The fixture now explicitly records that
macOS boundary and uses a representable name for its other checks, without
claiming non-Unicode round-trip evidence on that volume. Linux still requires
the actual non-Unicode creation/observation. Native four-platform execution of
the new cases is pending the next exact-head run. No cwd mutation or writable
resource is used by the product primitives. Filesystem discovery/core run and
the Windows captured-base integration remain unimplemented by this slice.

At exact head `b9bcf3e886f1e74804633c226fd67bb31014c8b6`,
[Unix run38835761382](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38835761382)
passed all seven jobs: native filesystem30 on each Mac and26 on each Linux CPU,
zero failures/ignores/filters. Both Macs explicitly logged their invalid-UTF8
EILSEQ boundary; both Linux targets executed the native-byte fixture without that
fallback. Other-UID namespace checks, serial process18, core request9/diagnostic3/
unit15/source6 and request decoder16 remained successful. The
[Windows run38835761343](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38835761343)
again failed only its actual creation gate on each CPU: non-gate library21 passed
with one separately executed gate filtered; gate0passed/1failed/21filtered.
No Windows source changed in this POSIX slice. Raw logs remain in owned target
scratch. This closes native POSIX selection primitives only, not full collection.

## Native output-path transport checkpoint

The frontend library now provides a borrowed Serialize-only `PathValue`, with
lossy display and separately encoded native units. The shared native codec lives
in path.rs; request.rs retains complete-document shape and resource admission.
Encoding strings and unit widths have one authority for both directions. There
is no filesystem operation, path resolution or business-policy copy in this
transport. The decoder's map-only/string-only restrictions remain intact.

Three independent tests assert hand-authored ASCII and raw-unit wire values,
preserved requested spelling, escaped embedded LF/ESC and raw Unicode line
separators. Windows ASCII Base64 was checked independently from explicit UTF-16LE
bytes using the platform Base64 tool. All19 local path/request tests passed with
zero failures/ignores/filters after the codec move. Local and Windows MSVC
all-target Clippy passed; the latter is compile-only. Both native workflow
families now explicitly select path3 and request16; the Windows guard requires
two positive unfiltered suite summaries. Pinned actionlint1.7.12 with
ShellCheck0.11.0 passed both changed workflows. Native six-platform execution
remains pending. Human terminal escaping, event/run maximums, LF event framing,
execution and the CLI binary are not implemented by this capability.

At exact head `2ec0456d674ee6e7cbe092e6e84d367d67a4dddb`,
[Unix run38836627963](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38836627963)
passed all seven jobs. Each of four Unix targets executed path3 and request16
with zero failures/ignores/filters, including all three new test names. The
[Windows run38836628309](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38836628309)
passed the same two suites on both native CPUs, exercising their UTF-16LE and
unpaired-surrogate oracles. Both positive-suite guards succeeded. Windows
non-gate filesystem21 passed with one filter; each separately run creation gate
still failed0passed/1failed/21filtered and remained the only failed step. The
Unix filesystem30Mac/26Linux and earlier independent suites stayed successful.
Raw logs are retained under owned target scratch. This closes native path
transport execution only; full protocol events/run size, CLI, archive and GUI
qualification remain open.

The separate human-path formatter now escapes terminal controls, Unicode line
separators and bidi-control marks without changing any native value. Two new
tests use a hand-authored escaped-line oracle, printable Unicode/backslash/joiner
oracle and all C0/C1/line/bidi control rejection cases. Local path5 and request16
passed without failures/ignores/filters; native-Mac and compile-only Windows MSVC
all-target Clippy passed. There is no shell-quoting, full human-event or stdout
delivery claim. Native six-platform execution of this formatter remains pending.

At exact head `8b874a61209a93338729a9f6d50dc0f81b4713ec`,
[Unix run38837652481](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38837652481)
passed all seven jobs. Path5/request16 passed without failures/ignores/filters on
each native Unix target. The
[Windows run38837652427](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38837652427)
executed the same path5/request16 successfully on both native CPUs; both formatter
test names are individually present on all six targets. Raw logs retain twelve
formatter test passes, two per target. The Windows non-gate library21 still
passed with one filter; each actual creation gate failed0passed/1failed/21filtered,
remaining the only failed step. This closes tested path-display primitives,
not full human events, actual stdout delivery or CLI execution.

## Existing-only Windows child lookup checkpoint

The native mechanism follows the separately challenged synchronous-existing-open
contract in windows-publication.md. It reuses the original Win32 root handle,
validates one native component and fixes both access/options selections inside
the private adapter. Observation exposes positive reparse kind/full identity
before ordinary-file metadata; directory open refuses reparse processing. The
future collector must compare the returned directory with its earlier observation
before descent. No complete collector, source-data reader or writable backend is
provided by this slice.

Three real filesystem cases are prepared for both native Windows CPUs under the
existing restricted runner token: full entry/signature/child IDs and native units,
held-parent lookup after an owned folder rename, positive reparse entry distinct
from its target/refused descent, pre-call invalid-component/extent rejection and
real missing-child error domains. One separate native translation-adapter case
uses a literal known status/code/display oracle. It is not a synthetic open or
real-filesystem integration substitute. Own test challenge avoids assuming child
file timestamps are unchanged merely because an ancestor was renamed.

Source review challenged an exact-zero success predicate and speculative closure
of failed output handles, then retracted those proposals against primary
NT_SUCCESS guidance and ownership documentation. The code retains nonnegative
success/informational semantics, with synchronous completion already established;
it never infers success from IOSB initialization. The valid independent error
oracle finding was implemented. Independent production/security and command-
central source reviews closed with no remaining findings after stable MSVC
Clippy/diff verification. Native execution remains pending; all actual creation-
gate failures remain in force.

Windows MSVC all-target check/Clippy passed as compile-only preflight. Local Mac
filesystem30 passed without failures/ignores/filters; doctests0, local Clippy passed.
Adding the generated Wdk_Foundation feature introduced no package/version or lock
change. Complete locked/offline fuzz metadata resolves27 packages with eml-fs;
this is graph admission only, not another fuzz campaign.

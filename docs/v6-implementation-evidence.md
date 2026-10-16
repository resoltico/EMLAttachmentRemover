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

At exact head `1fc670dbf9ddb4dcb46eb367889bc53d42e7b339`,
[Unix run38839554274](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38839554274)
passed all seven jobs. The
[Windows run38839554261](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38839554261)
linked and executed the generated NtOpenFile/Rtl adapter on both native GNU LLVM
CPUs, but the non-gate library suite FAILED24passed/1failed/0ignored/1filtered
on each. The failing held-parent test reached inventory.rs293, the fixture's
`std::fs::rename(selected,moved)`, and received Win32 access-denied5. All initial
child/full-ID/signature/native-unit/descent assertions before that line completed;
post-move assertions were unrun. The other three new cases passed, including
positive reparse observation/refused descent and the fixed adapter oracle.
The original creation gate also actually failed0passed/1failed/25filtered on
both. No passing complete child-integration claim follows from these results.

Independent challenge identified an unnecessary fixture precondition: retaining
the finished descendant directory while attempting to move its ancestor. The
narrow repair moves that child's drop before rename, leaving the original root
continuously held and preserving all successful-rename/old-address-gone/post-move
lookup/reopen-versus-earlier-ID assertions. Restricted actor and all production
rights/options remain unchanged. Whether lifetime alone explains native5 is a
hypothesis until rerun; no ACL or sharing cause is inferred from its numeric code.
MSVC all-target Clippy passes for the repaired fixture; actual both-CPU rerun is
pending. Failed raw logs remain in owned target scratch.

At exact head `6121fbce099c6618e298f7756dbdab33bd879f43`,
[Windows run38840175703](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38840175703)
passed the full non-gate library25/0failed/0ignored/1filtered on ARM64, including
the repaired held-root case. x64 still failed24passed/1failed/0ignored/1filtered
at the same fixture rename, now inventory.rs296, again native5. Thus releasing
the descendant alone did not resolve that x64 fixture precondition. Neither an
ACL cause nor universal rename behavior is proved. The other three new cases
passed on both CPUs; creation gates still failed0passed/1failed/25filtered each.
[Unix run38840175678](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38840175678)
completed successfully. Both failed Windows raw logs are retained.

The next independently reviewed fixture correction restores the known original
runner context only for moving its owned fixture, then enters a freshly verified
restricted token on the same thread before any post-move identity/observation/
open/enumeration. The original root remains held throughout. Every actual product
operation remains under a restricted actor; no restricted-actor rename authority
or exact token-object continuity is claimed. All earlier-ID and post-move oracles
remain. `try_exists().unwrap()` now makes an unexpected metadata failure visible
rather than treating it as absence. Production rights/options and the creation
gate remain unchanged. Source QA and stable Windows MSVC all-target Clippy passed;
actual both-CPU execution of this correction remains pending.

At exact head `bec62ecf758e4b405b184eec0d4ed819209a98c9`,
[Windows run38841250617](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38841250617)
passed the full non-gate library25/0failed/0ignored/1filtered on both native CPUs.
All four child/adapter test names passed individually, including held-root lookup
after the owned move with verified restricted contexts on each side. This closes
that tested read-only capability. It does not establish an ACL explanation for
the earlier denial, uninterrupted token-object continuity, creation confinement
or publication. Each actual creation gate still failed0passed/1failed/25filtered
and remained the only failed step. The
[Unix run38841250778](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38841250778)
passed all seven jobs; filesystem30Mac/26Linux and earlier independent suites
remained successful. Failed and successful raw logs remain in owned scratch.

## Unconditional child observation

POSIX `observe_child` now always observes one component with SYMLINK_NOFOLLOW,
matching the Windows no-follow child API. The selectable follow branch is removed
without a compatibility wrapper. Explicit-follow alias coverage calls the already
implemented `observe_selected` capability. All actual tests and five namespace
example call sites are migrated. Existing link-kind and selected-follow oracles
remain independent. Source QA closed; local filesystem30 and all-target Clippy
passed without failures/ignores/filters. No tests were added solely for the removed
flag. Native four-platform execution of the changed callers is pending the next
exact-head run; the collector itself remains unimplemented.

At exact head `c9d8a1f4fa3ff689dc8bc8b1c730a39e6de9d627`,
[Unix run38841852880](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38841852880)
passed all seven jobs: Linux26/Mac30 filesystem suites on both CPUs without
failures/ignores/filters, plus the actual other-UID example callers and earlier
independent suites. The
[Windows run38841852922](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38841852922)
again passed non-gate25/0failed/0ignored/1filtered on both CPUs; only the actual
creation gate failed0passed/1failed/25filtered each. Raw logs retain the pinned
head and counts. This closes the no-follow caller migration, not publication.

## Owned empty Windows directory primitive

The reviewed root-scoped writer acquisition and exclusive empty-directory create
are now implemented in eml-fs. Native inheritance remains null-descriptor/default;
there is no candidate file, message write or publication. Root/child write and
flush capabilities are explicit, exact FILE_CREATED establishes new-object
ownership, and sync receipts remain separate from creation and owned cleanup.
Cleanup checks native disposition, explicit close, held-parent validity and exact
missing-entry evidence; a positive matching residual is distinguished from an
unverified/replaced/delete-pending result. No recursive or fresh-path deletion
is performed. The earlier overstatement of continuous metadata immutability in
the addendum was corrected against the original contract, with all required
pre-edge/final checks and inherited-ACL/forced-stop guarantees preserved.

Three new native cases are prepared: held-root use after a prior fixture move,
exclusive create/collision/invalid components/wrong parent ID and unchanged
marker, and nonempty cleanup retaining positive owned IDs without recursion.
Child/parent directory flushes must actually succeed;145 is an expected native
nonempty-refusal oracle, not an execution result yet. Tests set only each fresh
owned temporary root's ACL to a current-UserSID full inheritable ACE, before
verified restricted actors perform every product operation. This proves neither
separate-user login nor a product ACL-admission policy. The old gate is unchanged.

The shared native diagnostic is now NtStatusError because both actual NT open
and create operations use it; exports/callers and the independent literal adapter
oracle are migrated without a compatibility alias. Shared metadata and actor
precondition checks are reused; the original four read/status cases remain.

Independent source/API/security/cleanup QA closed after one dependent-build
finding was fixed. The standalone filesystem probe's Windows all-target check
initially failed E0432 because a shared cfg(test) helper imported an undeclared
Authorization feature; the earlier non-test check did not cover that target.
The ACL fixture helper was localized to its only consumer rather than adding
an unused dependency to the probe. An intermediate missing-local-import E0425
failure was corrected. The exact standalone all-target check then passed, followed
by eml-fs Windows MSVC all-target Clippy. Those are compile-only preflights.
Local Mac filesystem30/Clippy passed without failures/ignores/filters; doctests0.
The included helper was explicitly formatted and checked. Complete locked/offline
fuzz metadata resolves27 packages; all three locks retain identical versions and
contents. No new fuzz campaign is claimed. Actual both-CPU execution of the three
new directory cases remains pending, as do changed-parent confinement, candidate
data/publication and all still-open product/archive/GUI requirements.

At exact head `bdf4d6b9e5514a018e9e14dcc599204d43dde9d6`,
[Windows run38849482570](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38849482570)
FAILED25passed/3failed/0ignored/1filtered on each native CPU. All three new
writable cases fail at writer acquisition (writable.rs474/516/562), plain Win32
access-denied5. No new NtCreateFile directory call, child/parent flush,145 or
cleanup oracle was reached. All prior25 read/status cases passed. Each old
creation gate also actually failed0passed/1failed/28filtered. The
[Unix run38849482579](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38849482579)
completed successfully. Source/compile QA was not native qualification; failed
raw Windows output remains in owned scratch.

The next reviewed diagnostic preserves production access/share/flags exactly.
Test-only messages distinguish native ReOpenFile failure from post-reopen
metadata/identity inspection. Each existing case also performs a same-context,
same-rights OPEN_EXISTING CreateFileW permission control on the same owned
fixture, compares its full ID with the earlier observation, then closes that
control handle before the original tested factory. It creates nothing, supplies
no production fallback and cannot substitute its success for factory success.
All original assertions/test counts and old gate remain. Root and independent
source QA plus Windows MSVC all-target Clippy passed for the diagnostic; actual
both-CPU diagnostic results are pending. No ACL/share/API cause is inferred from
numeric5 alone.

At exact head `3df0568526941ec19de46991437d5687aef0ae5a`,
[Windows run38850947528](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38850947528)
recorded three successful direct writable opens and full-ID metadata controls
per CPU, and three native ReOpenFile failures per CPU before any post-reopen
metadata check. The controls used the same verified actor, fixture, access,
sharing and flags, and closed before the original factory. All six ReOpenFile
calls returned native5; this identifies the failing call without claiming a
universal API limitation or a particular internal cause. Nongate25passed/3failed/
0ignored/1filtered and oldgate0passed/1failed/28filtered remained on each CPU.
No new child create/flush/cleanup was reached. The
[Unix run38850947477](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38850947477)
passed all seven jobs. Raw outputs remain in owned scratch.

The reviewed replacement removes ReOpenFile and its temporary controls/diagnostics
entirely. `bind_resolved(path, earlier_id)` deliberately binds the caller's
already-resolved ordinary root once with OPEN_EXISTING and the exact successful
control rights/share/flags. Type/tag/full identity must match before any creation.
A vanished old operand refuses; an explicitly supplied current moved operand is
a separate request, never discovery/rebaseline/fallback. Static selected-link
coverage captures its earlier target ID through read selection, requires writer
refusal of the unresolved link, then binds the independently known fixture target.
This does not prove GetFinalPathName, consistent ancestry or final-address admission;
all remain full-backend work. Mirrored children cannot use this path factory.
Relative child creation, sync, cleanup and the old failed gate are unchanged.

Root and independent production/test source QA closed without findings. Windows
MSVC all-target Clippy and diff verification passed. The previous writable API
has no compatibility alias, and no unused alternative remains. Actual both-CPU
execution of the replacement and its creation/flush/cleanup/145 oracles remains
pending; successful permission controls are not product qualification.

At exact head `1a26d957d295f76821aae4c5be610012864aab8b`,
[Windows run38852160394](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38852160394)
passed all three named writable cases once on each native CPU: nongate28passed/
0failed/0ignored/1filtered. Their executed assertions establish actual exclusive
FILE_CREATE ownership, ordinary/full-ID checks, child and parent FlushFileBuffers,
Complete empty cleanup, and native145 nonempty refusal with matching-ID Residual
without recursion. Earlier ID, stale-operand and unresolved-link refusal/explicit
known-target checks also ran. Each old creation gate still actually failed
0passed/1failed/28filtered, remaining the sole failed Windows step. The
[Unix run38852160326](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38852160326)
passed all seven jobs. Independent raw-log QA closed this bounded empty-directory
capability only. It does not qualify changed-parent confinement, candidate data,
publication, GetFinalPathName/ancestor/address admission or the full product.
Prior failed runs remain recorded; no universal ReOpenFile cause was inferred.

## Owned Windows file staging checkpoint

The Windows storage-only capability now creates an owned regular file with native
inheritance/share-zero, writes/rereads/compares through that same synchronous
handle in bounded chunks, probes EOF, requests file flush and validates final
type/tag/full ID/length. ReadyCandidate construction/handles remain private and
it has no publication operation. StageFailure keeps primary operation/error,
actual sync and cleanup independently. Post-flush cancellation/storage validation
retains Some(Succeeded) with Write classification, rather than inventing a failed
flush. Owned nonempty files use handle deletion; directories retain native
empty-only deletion. Residual proof now compares direct full identity; inventory's
richer observer remains intact.

The128-bit generator and16-attempt constant moved to one shared source for the
two actual native storage consumers. POSIX storage/privacy/publication mechanisms
are unchanged. Windows retries only exact native collision with NotNeeded cleanup,
retains its last actual collision on exhaustion and propagates generator/other/
Unknown failures. The existing getrandom version became common; complete locked/
offline fuzz metadata resolves27 packages and all three locks remain unchanged.
This is graph verification, not a new fuzz campaign. StageOperation comments now
distinguish POSIX private resources from Windows inherited ACLs.

Three new Windows cases are prepared: real nonempty/zero storage with independent
positioned readback, actual normal read/write-open exclusion and cleanup;
before-create/observed-partial-write cancellation plus post-flush failure mapping
with the SAME genuinely flushed owner/receipt; and16 real native collisions with
unchanged marker, scripted generator failure and validation-based no-retry. The
fixture helper is shared only by its two FS test consumers. No candidate metadata
attack, final rename, ACL-policy framework or gate replacement is added.

Independent source/API/I/O/test/security reviews closed without concrete findings.
Windows MSVC FS all-target Clippy, standalone probe all-target check, local Mac
filesystem30/Clippy and diff verification passed; doctests0. Initial unused-import
findings were fixed without suppression; stable-input checks then passed. No
Windows runtime case is yet claimed. Zero-length payload is covered by the new
case, but short-write/WriteZero/EINTR return paths are source-reviewed, not
deterministically exercised. Scripted generator refusal is not an OS RNG failure;
post-flush cancellation is not a native flush-failure test. These limits remain
explicit rather than introducing broad mock scaffolding or claiming exhaustive
failure coverage. Both-CPU native execution and the unchanged failed confinement/
publication/product/archive/GUI requirements remain open.

At exact head `543f8a73cbdaa94c7f6fcb95df7b3834befd8c6c`,
[Windows run38854647778](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38854647778)
passed all three named storage cases once on each native CPU: nongate31passed/
0failed/0ignored/1filtered. Executed assertions establish real nonempty and zero
storage with independent positioned readback, file FlushFileBuffers, normal
read/write-open exclusion32, Complete cleanup, before-create/observed-partial-write
cancellation, post-flush mapping with a genuinely flushed owner/receipt,16 native
collisions and unchanged marker, scripted generator refusal and validation-based
no-retry. None of these substitutes for native short-write/WriteZero/EINTR or
flush/RNG-failure evidence. Each old creation gate still actually failed
0passed/1failed/31filtered, the sole failed Windows step. The
[Unix run38854647761](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38854647761)
passed all seven jobs, including shared POSIX generator changes and earlier
read/directory regressions. Independent raw-log QA closed bounded storage only.
Active directory/storage documentation now points to these verified capabilities;
full admission, changed-parent confinement, publication, archives and GUI remain
unqualified. Raw logs stay in owned scratch.

## Consuming Windows physical rename edge

The separate design/challenge pass closed through command central and independent
security/API/receipt review. Ready storage consumes into a single physical rename
attempt using generated FILE_RENAME_INFORMATION, NULL root, one basename and zero
replace flags. Actual parent-handle volume component limits, checked arithmetic,
aligned storage and the API-specific synchronous completion guarantee replace
guesses and the earlier standalone probe's pending/abort/IOSB handling. The
private post-attempt owner alone determines effect; Published/Unknown can only
checked-close, never set disposition or regain a staging/publish capability.
Original native rename errors and release/cleanup observations remain separate.

Three benign native cases are added for complete held bytes/full identity at the
final basename and retention after release, unchanged conflict winner/owned loser
cleanup, invalid/actual-limit pre-edge refusal, and explicitly scripted Unknown
mapping on real storage. That mapping is not an actual ambiguous OS return or
native close-failure test. Core stop integration, namespace/final-copy/address
qualification, changed-parent confinement and Created remain unimplemented.

Stable-input local checks passed: MSVC FS all-target Clippy with warnings denied
(compile evidence only), excluded native probe all-target check, Mac FS30 tests,
Mac FS all-target Clippy, formatting and diff checks. Doctests0. Source QA and
native two-CPU edge execution are pending; the old failed gate is unchanged.

Independent production/API/ownership/security source review closed without
material findings. Test review identified a missing positive boundary control;
the amended native case now requires exact actual component-limit publication,
retained full ID and independent literal-byte readback after release, beside a
fresh one-over stage that must refuse and clean. Independent held-file readback
also loops through EOF rather than assuming a full single read. The narrow test
review and independent MSVC Clippy/Mac FS30 checks closed against committed
`d58d503b417e73f1058f7d24d81d981017cd9364`. Native raw-log review remains pending.

The separate proposed process-scan change (ignore every global vanished PID at
getsid/ESRCH) was rejected in challenge: an unobserved same-session successor can
be born after the census while its listed parent exits, repeating across two
scans. Prior/pinned-group knowledge cannot classify never-observed descendants.
This is a source-level design counterexample, not an executed fixture or proof
of the earlier parallel failure's cause. No process code or test changed.

At exact head `d58d503b417e73f1058f7d24d81d981017cd9364`,
[Windows run38856809628](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38856809628)
passed each of the three new edge cases once per native CPU; nongate34passed/
0failed/0ignored/1filtered on each. Actual assertions cover complete same-held
bytes/full ID at the final basename, retained final data after checked release,
exact native collision and unchanged winner/owned loser cleanup, invalid-name
refusals, actual exact-limit acceptance and one-over refusal, plus labelled
scripted Unknown mapping. No native ambiguous return/close-failure or Created
claim follows. Independent Windows full-head/raw-log review closed this bounded
edge evidence. Each unchanged old gate actually failed0passed/1failed/0ignored/
34filtered, the only failed step in each Windows job; overall run remains failed.
[Unix run38856809639](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38856809639)
passed all seven jobs at the same head, including Mac FS30/Linux FS26, real
different-UID namespace controls and serial process18. Raw logs are retained at
target/d58d503-windows-ci.log and target/d58d503-unix-ci.log. Full Windows
admission/confinement, namespace/final-address qualification and product gates
remain open.

## Explicit Windows working base and isolated process scenarios

PathContext::capture now consumes the caller-supplied absolute working expression,
with every existing FS/core test caller migrated and no compatibility constructor.
Ordinary bases retain Win32 normalization. The reviewed previously unspecified
mixed rule preserves literal name dots/spaces under a verbatim DOS/UNC base while
standard Rust supplies root-clamped dot navigation. Other drives retain captured
state; requested terminal separators and original native units survive. This is
not ordinary Win32 or full .NET path equivalence, and adds no1703 API/floor change.

Source challenge caught three concrete gaps, all repaired before native push:
bare C: must retain its base without an added separator (including the32,766-unit
operational boundary); ordinary explicit bases must actually override ambient
cwd (distinct owned first/second bytes and raw operand oracle); malformed UNC
introducers must refuse instead of rebasing to local root-relative operands.
Two added tests cover independently authored DOS/UNC/native-unit/separator cases
and real owned literal-base bytes/full identities. UNC string cases do not prove
a network round trip; operational-length cases do not prove filesystem support
for that component. Source/API/caller/test/docs QA closed; native execution is
pending. Full root/ancestor/address admission remains separate unfinished work.

The process integration harness now holds a test-local mutex from the first
statement of each of18 outer scenarios through resource Drop. All assertions and
concurrency within scenarios remain; no production inspector, signal, refusal,
unknown-state or cleanup budget changed. Independent design/source review closed
this as shared global-census/resource isolation, not a simultaneous-controller
or ambient-churn guarantee. Native CI removes its redundant test-thread flag to
exercise the same harness isolation. Prior9pass/9fail default-parallel evidence
and the subsequent separate serial observations remain historical observations;
these changes do not retrospectively prove cleanup of an earlier unknown result.

Local stable-input default process invocation passed18/0failed/0ignored/0filtered
in13.65s. Full `cargo test --locked --workspace` then passed171 tests across14
positive suites, with0failed/ignored/filtered; six expected empty suites include
doctests and platform-empty harnesses. Its process suite passed18 in14.18s.
Raw logs: target/process-scenario-isolation.log and
target/workspace-scenario-isolation.log. MSVC FS/Core all-target Clippy, excluded
native probe all-target check, local Mac FS30/Clippy, xtask all-target Clippy,
changed-workflow actionlint, formatting and diff checks passed. Compile-only
Windows evidence is not native execution. Native matrices, the old failed
confinement gate, full product/archive/GUI and final clean-clone qualification
remain open.

At exact `bc1d102c023aa7fe4b9f230a7d77b4a281bf31c6`,
[Windows run38858057646](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38858057646)
passed both new path cases once per CPU and the strengthened isolated-child and
parent suite (interleaved stdout is retained). Nongate36passed/0failed/0ignored/
1filtered on each. Independent full-head/raw review closed the bounded explicit-
base/literal-name/identity/separator/root/drive/native-unit/malformed-UNC checks;
no live UNC-share round trip follows from string cases. The actual old gate still
failed0passed/1failed/0ignored/36filtered on both, sole failed step; overall Windows
remains failed. [Unix run38858057577](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38858057577)
passed all seven jobs. Default process harnesses passed18 each without an external
serial flag (Linux9.15s/Mac14.44s); FS Mac30/Linux26 and earlier boundaries passed.
Raw logs: target/bc1d102-windows-ci.log and target/bc1d102-unix-ci.log. This
establishes isolated scenario execution and bounded path behavior, not parallel-
controller, ambient-churn or full product qualification.

## Windows explicitly selected root metadata

The reviewed free `observe_selected` operation accepts an already admitted
absolute extended DOS/UNC expression and reuses fixed existing-only Win32 open
with READ_ATTRIBUTES, share-all and BACKUP_SEMANTICS, following the selected final
link. Its temporary handle supplies existing kind/full-ID/signature observations,
with no data access, writes, privilege enablement or new native ABI. Exact failed
queries/unusable identities remain errors; non-disk is Unsupported and remaining
reparse attributes remain Reparse. Terminology now says metadata observation,
since separate queries do not provide an atomic snapshot. Later root opening
must compare the earlier ID; physical admission and the core collector remain
unfinished.

Two benign owned native cases cover unpaired-name file metadata versus BoundFile,
directory ID versus BoundDirectory, exact missing error2/invalid-input controls,
and static explicit-directory-alias target versus no-follow walked Reparse with
distinct entry ID/refused descent. Shared helper comparisons establish selection/
API compatibility, not independently decoded metadata. Static link setup precedes
observation; no new mutation/reproduction fixture is added. Independent design/
API/source/test/docs review closed with no findings. MSVC FS all-target Clippy,
excluded native probe all-target check and local Mac FS30/Clippy passed;
doctests0. Native two-CPU runtime checks remain pending and the old gate unchanged.

At exact `cbc9947b0286f36d61b0e596d73843dbf478a48c`,
[Windows run38858860874](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38858860874)
passed both selected-root cases once per CPU; nongate38passed/0failed/0ignored/
1filtered each. Independent full-head/raw review closed the bounded metadata/
selection compatibility evidence, including unpaired name, size23/full signature/
ID, missing native2, relative/NUL refusals and static selected target versus
no-follow Reparse/distinct ID/refused descent. This is not independent metadata
decoding or full root admission. Each actual old gate still failed0passed/1failed/
0ignored/38filtered, sole failed step; Windows overall remains failed.
[Unix run38858860876](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38858860876)
passed all seven jobs at the same head; independent raw QA closed FS Mac30/Linux26,
default process18 each (Mac12.12s/Linux9.97s), prior boundaries and real cross-UID/
ACL controls. Raw logs: target/cbc9947-windows-ci.log and target/cbc9947-unix-ci.log.

## Windows resolved directory name observation

BoundDirectory::resolved_path adds the required narrow normalized DOS-name query
using the held ordinary directory. It derives one32767-WCHAR buffer from the
existing32766-content authority, retains actual zero-return native errors,
refuses capacity results without retry/fallback, preserves UTF-16 units and
reuses fixed extended-path validation. Held ordinary/full-ID checks bracket
success. This supplies a name observation, not an ancestor/address/overlap
qualification or permission to create through that name.

The static selected-junction case now checks the RETURNED operand with final
no-follow metadata and requires the known ordinary target ID. Original alias
observation is Reparse/distinct ID, a rejecting control that would catch merely
returning an unresolved alias. One new native case queries a greater-than260-unit
owned directory with an unpaired UTF-16 final name and compares returned units,
ordinary no-follow identity and a separate reopen. Temporary metadata files are
explicitly dropped before owned fixture teardown. No mutation fixture was added.

Independent design/API/source/test review closed without findings; independent
MSVC FS all-target Clippy and excluded native probe all-target check passed.
Local Mac FS30/Clippy, formatting and diff checks passed; doctests0. Native two-CPU
execution remains pending. Full collector/report/physical-overlap decisions and
Windows ancestor/final-copy qualification remain unfinished; the old gate is
unchanged and failed.

At `31dd42aa9c3311f7a9c7fa7bd09f51d95c37f4a6`,
[Windows run38861257739](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38861257739)
passed the strengthened static-alias and new long/unpaired resolved-name cases
once per CPU; nongate39passed/0failed/0ignored/1filtered. Independent full-head/raw
QA closed only name-observation/native-unit/ordinary no-follow returned-target
evidence. Native maximum-buffer/error/SMB failure and complete ancestor/address/
overlap behavior are not established. Each actual old gate failed0passed/1failed/
0ignored/39filtered, sole failed step; overall Windows still failed.
[Unix run38861257723](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38861257723)
passed all seven exact-head jobs; independent raw QA closed Mac FS30/Linux26,
default process18 each, prior boundaries and real cross-UID/ACL controls. Owned
full logs are target/31dd42a-windows-ci.log and target/31dd42a-unix-ci.log.

## Separate reported native and display expressions

Independent contract/challenge review resolved the walked literal-name case:
Windows reported NativeValue uses the admitted extended SELECTED expression plus
literal suffix, preserving aliases; original request-derived expression remains
the display fact. A canonical target name is a separate binding observation.
Unadmitted failed roots may retain intended failed input when no usable operand
exists, never as a successful path capability. Charge each source/known destination
by max(complete display-expression native bytes, complete reported-native bytes),
including failures, repeated logical prefixes and expanded reported cwd. The
existing display/native envelope proof holds; this does not bound two independently
allocated buffers to8MiB. Collector/maximal/exact-one-over/native integration
enforcement remains to build.

Reviewed sequencing now requires physical overlap admission before writes, rather
than the added before-walking restriction. OUTPUT_OVERLAP has no item events and
actual discovery completeness; INPUT_ADMISSION_FAILED separately remains false.
Metadata-only root observations do not establish work precedence or backfill
collection stop/limit prefixes. Native mount-alias negative proof remains open;
no filtered traversal or upward inode no-match is called disjointness.

PathValue now requires both borrowed core-supplied paths, renders display from its
display expression and losslessly encodes the native value. Every previous caller
passes same/same; the additional hand-authored A/request-alias golden requires
Unix QQ== or UTF16LE QQA= without deriving native bytes from display. Request codec
and terminal escaping are unchanged. Independent source/API/test/docs QA and local
CLI six path/sixteen request tests, native-host and MSVC all-target Clippy, formatting
and diff checks passed. Native six-platform codec execution remains pending; no
collector, event/run sizing or split-path filesystem integration claim follows.

At exact `b814ee72f5b748da5ec13c933e7db05a34ad05e6`,
[Windows run38862917526](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38862917526)
and [Unix run38862917415](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38862917415)
executed the split-fact golden once on all six native hosts. Path6/request16 each
passed0failed/ignored/filtered; independent exact-head/raw QA closed this codec
checkpoint. Unix all seven jobs succeeded, including prior FS/core/cross-UID/ACL
boundaries and default process18. Windows FS nongate39 passed each; actual old
gate0passed/1failed/0ignored/39filtered remained the only failed step per CPU, so
Windows overall remains failed. Full retained logs: target/b814ee7-windows-ci.log
and target/b814ee7-unix-ci.log. No collector/native split-path I/O/maximal-event
qualification follows from a serializer golden.

## Read-only Windows volume name limit

The unchanged positive-DWORD/zero/native-error/WCHAR query now has one private
handle helper, shared by public BoundDirectory::name_limit and Creation's retained
parent. Publication keeps its own fresh query. No cache, rights escalation,
fallback or new FFI is introduced. The existing native boundary test calls the
read-only getter before constructing a write capability, compares an independently
issued direct native DWORD, then uses public Ready.publish for exact/max+1,
retained identity/content and owned cleanup controls. The superseded private-owner
extraction through an invalid-name attempt is removed.

Independent design/API/source/test review and FS MSVC all-target Clippy/excluded
native probe all-target checks closed without findings. Local Mac FS30/Clippy,
formatting and diff checks passed; doctests0. Native runtime for this read-only
property/public-flow amendment is pending. Metadata availability is not full
write admission, overlap or confinement qualification; the old failed gate stays.

At exact `e36855b620ac1be393226f51c5051e29c388ddd1`,
[Windows run38866089872](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38866089872)
passed the amended read-only getter/direct native DWORD/public exact/max+1 flow
on both native CPUs. Nongate39passed/0failed/0ignored/1filtered; each actual old
gate still failed0passed/1failed/0ignored/39filtered, sole failed step. Independent
exact-head/raw QA closed bounded metadata availability, not full writer authority.
[Unix run38866089881](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38866089881)
passed all seven jobs, preserving FS30Mac/26Linux, codec6/request16, earlier core/
source and real cross-UID/ACL controls, default process18 each. Full retained logs
are target/e36855b-windows-ci.log and target/e36855b-unix-ci.log. This head has no
collector implementation; it cannot qualify the following change.

## Read-only core collection and output separation

The reusable collect_sources operation now owns request validation, captured
context/output prerequisites, ordered bounded inventory, exact-spelling/anchor
dedupe, explicit/descendant provenance and unfiltered explicit-output separation.
Requests are truly borrowed; filesystem handles and authoritative state remain
private. Ok means an outcome exists, not complete discovery or write permission.
RequestPathError is the pure path/count/NUL/absolute-base error, migrated through
all real core/decoder callers. Outer InvalidRequest owns actual prerequisite I/O
causes through Error::source. Source-root failures after raw validation remain
returned INPUT_ADMISSION_FAILED; no classification parses error text or infers a
change from ErrorKind alone.

Independent source challenge found and corrected complete Windows-operand
validation, unavailable-key dedupe, late mandatory root-diagnostic suppression,
completion/late-stop evidence loss, and stale-cwd cross-anchor coalescence. Complete
literal Windows operands reuse the existing verbatim validation branch after
append. Failed root diagnostics bypass work lookup/indexing; intended fallback
paths have no admitted key. Relative and absolute directory routes bind/walk
separately; only matching full native spelling plus actual leaf signature/parent
can coalesce cross-anchor file work. No directory-context interchangeability,
inode-global visited set, statx/proc/F_GETPATH expansion or new OS floor is claimed.

Controlled owned fixtures reject hidden/generated-branch false negatives, lost
late root failure after an earlier walked file, cwd replacement with distinct
leaves and with a hard-linked leaf/different parent, unsafe Path equality for
./a, unordered truncation, and forgotten prior errors/completed discovery on a
scripted late stop. Private hash injection controls collision/RunControl/ordinary
empty-directory transitions with actual filesystem I/O; it is not other-user,
mount, signal or native OS-fault evidence. Real native local tests execute exact/
one-over4096 work,64 depth and100000 directory entries. A Windows literal folder
case now uses non-dot folder. versus folder, retaining the production dot skip.
The pure combined Windows32766 boundary uses32757 x units plus7-prefix/2-join-leaf;
this is not a filesystem component-capacity claim. Positive observed output IDs
select overlap before a later depth/open refusal; the opened-ID check remains.

Final source/API/error/budget/test/workflow QA closed with those amendments.
Stable local full workspace ran192 passes across15 positive suites, zero failures/
ignores/filters, six expected empty suites (target/core-collection-workspace-final.log).
After the last arithmetic/positive-overlap amendments, targeted core unit24,
collection11, request9 and source6 passed with zero failures/ignores/filters
(target/core-collection-prepush-local.log). Host/MSVC core+CLI all-target Clippy,
excluded probe all-target check, formatting/diff and both changed-workflow
actionlint checks passed. Earlier lint findings were fixed, not suppressed.
Native workflows now explicitly discover collection alongside core units/source;
Windows requires three positive unfiltered suites. Expected native discovery is
24units/11collection on Unix and23units/12collection on Windows; runtime is pending.

This phase reads no message bodies, creates/flushes no output, changes no process
cwd and exposes no frontend write handles. Destination naming/layout/reservations,
full source/destination/ancestor/address integration, serial MIME execution,
typed terminal events/counts/delivery, actual CLI, SwiftUI, archives and final
qualification remain unfinished. The old Windows confinement gate stays failed;
no native mount fixture, saved preferences or unrelated work was changed.

At exact `dfc3c76c22e3a5bd45cfa7dbb7332a76fd027b12`,
[Unix run38877295410](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38877295410)
passed all seven jobs/full-head checkouts. Each of four FS hosts executed all
nine new collection unit names and all eleven integration names once as passing;
actual core24/collection11/source6/request9/diagnostic3/path6/decoder16 had zero
failures/ignores/filters. FS Mac30/Linux26, real cross-UID/ACL controls and both
default process18 suites remained successful. [Windows run38877295339](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38877295339)
executed every new expected case once on BOTH native CPUs, including combined-
operand validation, literal folder/source-native integration and depth/overlap
precedence; core23/collection12/source5/request9/diagnostic3/path6/decoder16 all
passed with0failed/ignored/filtered. Collection runtime was10.26s x64/165.39s ARM;
the real100000-entry case was executed, not skipped. FS nongate39 passed each;
each actual old strict gate still failed0passed/1failed/0ignored/39filtered, the
sole failed Windows step and all its panic/error annotations. Windows overall
remains FAILED. Independent exact-head/raw/discovery QA closed bounded read-only
collection only. Full retained logs: target/dfc3c76-windows-ci.log and
target/dfc3c76-unix-ci.log. This supplies no write authority, full product/CLI/GUI
or archive qualification.

## Read-only destination layout and reservations

The reusable plan_destinations operation consumes one collected inventory and
retains structural records at every known source position. Naming, root labels,
mirrored layout, parent observations, source reservations and all-contender
collision detection live in core. OperationFailure now carries the shared typed
code/phase/detail and exact native I/O cause. The planner stores one native path
per known destination and derives display paths from the borrowed request and
frozen root labels; it does not retain duplicate complete path buffers.

Independent design/source/test review corrected normalized nameless-root labels,
unverified-parent naming assumptions, duplicate path storage, and premature quota
charging on a stopped candidate. Known destination paths remain available through
leaf errors; unknown parent routes never acquire a speculative key. Native-only
optional-absence helpers preserve all failures except the exact native missing-
name result. The Windows classification unit test is a scripted mapping control,
not execution of injected OS failures. Existing final-leaf I/O-error handling is
source-reviewed; the blocked-file fixture proves incompatible mirror refusal.

Eleven real owned-filesystem tests cover layout, all-contender collisions,
missing-tail reservations, source aliases/hard links, truncation collisions,
collection/stop preservation, dot and nameless root labels, incompatible mirror
parents, and the combined8MiB report budget. The budget fixture independently
expects4,177,920 source bytes,4068 Unix/2034 Windows admitted destinations and
8,388,300 final bytes, retaining the full source list and unplanned suffix. Its
Windows prerequisite explicitly requires a normalized scratch prefix<=499 UTF16
units before the large fixture setup; this is fixture scope, not a product path
restriction. No output entries are created. Workflows discover all eleven tests
and Windows now requires four positive unfiltered core suites.

Stable local cargo test --locked --workspace passed204 tests across16 positive
suites, six expected empty suites, zero failures/ignores/filters, command exit0
(target/destination-workspace-final.log). Host workspace and MSVC three-package
all-target Clippy, both workflow actionlint checks, excluded native-probe all-target
check, formatting and diff checks passed. Native runtime for this slice remains
pending; MSVC compilation is not Windows integration evidence.
This supplies no MIME processing, existing-byte/sync comparison, writable ancestor
binding, final publication qualification, CLI/GUI or archive evidence. The prior
Windows confinement gate remains failed and unchanged.

At exact `246743aeb4a6ce86c8626c4001a10dad3bce40d6`,
[Unix run38884463382](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38884463382)
passed all seven jobs. Each native filesystem host executed all eleven destination
cases exactly once, core24/collection11/destination11/source6 and prior request9/
diagnostic3/path6/decoder16 passed0failed/ignored/filtered. FS Mac31/Linux27 passed,
including native optional absence; real cross-UID/ACL and default process18 each
remained successful. [Windows run38884463363](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38884463363)
executed all eleven destination cases exactly once per native CPU, including the
real combined-path quota and nameless-root controls. Core23/collection12/
destination11/source5/request9/diagnostic3/path6/decoder16 all passed with zero
failures/ignores/filters. FS nongate41 passed each, including actual optional
absence and exact native-status mapping controls. The unchanged old strict gate
still failed0passed/1failed/0ignored/41filtered per CPU, the only failed step and
source of panic/error annotations. Windows overall remains FAILED. Full raw logs
are target/246743a-unix-ci.log and target/246743a-windows-ci.log. Native planning
supplies no writer, terminal run, existing-copy synchronization, CLI, GUI or
archive qualification. Independent exact-head/raw/discovery review closed this bounded checkpoint.

## Read-only existing-copy comparison

Core compare_existing now takes the privately verified PreparedSource and an
already no-follow-opened BoundFile. It checks source/existing bindings, rejects
the current source identity, compares length then bounded64KiB reads through EOF
with a candidate-length-plus-one cap, and checks both bindings afterward. The
successful value owns the same existing file. Typed failures retain original
identity, optional completed equality observation and existing stability apart
from source/binding/data error. No native Display text supplies a wire diagnostic.
This is not UpToDate or synchronization/address qualification; final driver must
open no-follow and reject all selected/earlier-created identities independently.

Five owned-file tests on Unix/four on Windows exercise exact independent golden,
equal-length differences/short/empty/long files, actual hard-linked source alias,
initial stop, retained handle and unchanged content. Unix additionally changes
ordinary owned source/existing files before comparison and asserts distinct causes.
Five private reader controls establish short-read/chunk/EOF/+1 bounds, length-only
refusal, empty EOF, premature EOF/extra/different bytes, EINTR/stop precedence and
native data-error retention. These scripted controls are not native OS fault or
concurrent-writer evidence. Windows's actual excluded-writer check remains in the
owned matching-file case. Native workflows explicitly select existing integration
and require five positive unfiltered core suites on Windows.

Local targeted core29/existing5 passed0failed/ignored/filtered with command exit0
(target/existing-local.log). Host and MSVC core all-target Clippy passed. Stable full workspace passed214 tests across17 positive/six empty suites with
zero failures/ignores/filters, command exit0 (target/existing-workspace-final.log).
Formatting, diff and workflow actionlint checks also passed. Independent design,
source/test/workflow QA closed this read-only scope. Native comparison runtime
remains pending.
No file synchronization, output creation, CLI or frontend capability is claimed.

At exact `55f51707d17b6890e651bc7a227ec333a7737524`,
[Unix run38885524455](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38885524455)
passed all seven jobs. Each four-host filesystem run executed all five new
comparison unit controls and five actual Unix integrations exactly once; actual
core29/collection11/destination11/existing5/source6/request9/diagnostic3/path6/
decoder16 all passed0failed/ignored/filtered. FS31Mac/27Linux, real cross-UID/ACL
controls and default process18 on both probe hosts remained successful.
[Windows run38885524041](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38885524041)
executed all five new comparison controls and four actual integrations exactly
once on both native CPUs, including retained read-only handle writer exclusion.
Core28/collection12/destination11/existing4/source5/request9/diagnostic3/path6/
decoder16 passed with zero failures/ignores/filters. FS41nongate/1filter passed;
each actual old strict gate remained0passed/1failed/0ignored/41filtered, the only
failed step per CPU. Windows overall remains FAILED. Independent exact-head/raw/
discovery QA closed bounded read-only comparison. Full retained logs are
 target/55f5170-unix-ci.log and target/55f5170-windows-ci.log. This head does not
contain the subsequent dry_run driver or qualify resynchronization/publication.

## Serial read-only execution and typed outcomes

The real public dry_run library operation now consumes collection/planning,
reopens one stable source, uses the one MIME preparation engine, admits metadata,
and inspects/compares existing copies serially. DryRunModeRequired rejects a
Create request before I/O/events. No general Create run, CLI executable or GUI is
claimed. The final shared typed model separates intended paths, terminal results,
publication effects, existing knowledge, receipts and delivery. CreatedCopy and
ExistingCopy qualification values have no constructors until their real native
consumers exist; failed published evidence does not qualify a Created result.
Events borrow only current metadata; final RunOutcome retains counts, discovery,
interruption and run/delivery failure rather than message byte buffers/item lists.

Independent challenge corrected stale absolute POSIX cwd reopens, cancellation
coercion, destination-role error mapping, invalidated-limit/address confusion,
failed metadata inspection/presence confusion and same-ID signature/address
classification. Source parent binding now carries a distinct stop result; genuine
native failures remain native failures. Contextual parent rebinding compares
frozen prefixes and name assumptions without global cwd changes or live-absolute
rebasing. The final existing lookup distinguishes exact absence, different IDs,
same-ID signature/type changes and unverified lookup; reached byte observations
remain separate. Successful summary delivery fixes the returned semantic exit;
a late Stop cannot contradict its completed stream. Failed delivery never calls
RunControl, stops later work/emission and preserves predelivery terminal counts.

Eighteen actual Unix integrations/seventeen Windows cases cover MIME preparation,
mirrored Create plans, readonly VerifyExisting plans without sync, different-copy
conflict, gapped explicit-root failure, no inputs/overlap, real4097 collection
refusal without processing a subset, stop and delivery boundaries, all-contender
collision, strong current all-source identity reservation, mode/output admission,
owned source/output address transitions and exact20,000/+1 metadata admission.
Already reported failure metadata remains charged. Native name tests use actual
Linux non-UTF8 bytes, Windows unpaired UTF16 and macOS combining Unicode; they do
not claim Mac invalid-UTF8 creation. Unix additionally proves captured-relative
I/O after the cwd moves and its old pathname is replaced. These are controlled
owned event-boundary transitions, not concurrent interference/signal evidence.
Three focused unit controls cover typed stop/native-error classification, a
queried actual-volume name limit with a deliberately invalidated assumption, and
actual owned unchanged/missing/different-ID/same-ID changed entries. Their raw
error/type alternatives are labeled scripted classification, not OS fault or
file-to-directory conversion evidence.

Final stable cargo test --locked --workspace exited0:235 passes across18 positive
suites/six expected empty suites, zero failures/ignores/filters
(target/dry-run-workspace-final.log). After the final classification fix, focused
core32/dry_run18 passed0failed/ignored/filtered
(target/dry-run-final-classification.log). Host workspace and MSVC three-package
all-target Clippy, native-probe all-target check, workflow actionlint, formatting
and diff checks passed. Initial source/lint findings were fixed without waivers.
Independent source/API/test/workflow review closed with the corrections above;
Windows workflows require six positive unfiltered core suites. Expected native
execution is core32/dry_run18 on Unix and31/17 on Windows. Native runtime for this
slice remains pending; no cross-compilation result qualifies that gate.

No output directories, staging, publication or synchronization occur here. Real
new-copy/UpToDate qualification, Windows existing-file sync and ancestor/address
binding, CLI/GUI, packaging and required final archive/native/GUI gates remain
unfinished. The unchanged Windows creation gate remains FAILED; no overall
product/WP acceptance follows from this bounded read-only operation.

At exact `ac8e193c12c90cad80b5785929fb28defcd63f24`,
[Unix run38888468516](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38888468516)
passed all seven jobs. All18 dry-run integrations and3 new classification units
passed exactly once per filesystem host; actual core32/collection11/destination11/
dry_run18/existing5/source6/request9/diagnostic3/path6/decoder16 passed with zero
failures/ignores/filters. FS31Mac/27Linux, real cross-UID/ACL controls and both
process18 runs remained successful. [Windows run38888468345](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38888468345)
executed all17 dry-run cases and3 new classification units exactly once on both
native CPUs. Core31/collection12/destination11/dry_run17/existing4/source5 and
prior request9/diagnostic3/path6/decoder16 passed unfiltered without failures or
ignores. FS41nongate/1filter passed each; the actual unchanged old strict gate
remained0passed/1failed/0ignored/41filtered, sole failed step per CPU. Windows
overall remains FAILED. Independent exact-head/raw/discovery review closed bounded
read-only serial execution only. Retained logs: target/ac8e193-unix-ci.log and
target/ac8e193-windows-ci.log. This head contains no later resync primitives and
supplies no general Create, UpToDate, CLI/JSON, GUI, archive or writable-confinement
qualification.

## Windows existing-file flush access

The bounded filesystem prerequisite adds fixed BoundFile::open_for_sync,
actual sync_file and checked consuming close. Source/default opens stay read-only;
all primary owners retain shareREAD. Binding observation uses only READ_ATTRIBUTES
with compatible sharing, retaining parent/entry/held signature checks without
self-conflicting against the new owner's write access. Existing creation and
ordinary file owners share one unchanged checked CloseHandle implementation.

Independent design challenge corrected the initial proposal to request write
access before comparison: a readable readonly different destination must remain
a byte conflict. The approved concrete Core lifecycle is read-only comparison,
checked release of only that holder, fixed sync open, full binding/alias checks,
the same bounded comparison again, then actual file/namespace/address checks.
The selected source remains held. This primitive does not implement that Core
consumer or qualify UpToDate. ReOpenFile cannot bypass a still-open reader's
sharing restriction; SDK primary references are linked in windows-publication.md.

Three new Windows tests reuse the existing restricted same-UserSID actor and
fresh owned fixture ACL, without product ACL changes or another actor framework.
They exercise a readonly-handle flush access failure, checked RO closure and fresh
RW same-ID/signature/read/actual-flush/self-validation/checked closure while a
separate source stays held; an actual readonly-file access refusal with untouched
bytes/readonly flag and fixture-only restoration; and a live RO holder's32
sharing refusal followed by fresh sync access after checked close. No data or
metadata setter is exposed by the product primitive. They are compiled, not yet
executed natively at this checkpoint.

One additional all-platform serial test covers a benign static selected directory
alias through actual preparation and comparison: default VerifyExisting and
explicit-root Create plan preserve selected alias spelling, unchanged original/
existing bytes and empty output. It shares the established provider setup helper
with collection tests, with no profile/path interpolation/privilege adjustment or
post-bind alias mutation. The earlier suspected parent-alias defect was retracted
on reading the actual follow-final observation flags; no canonical I/O rebase was
implemented and this test does not claim such a defect was fixed.

Stable cargo test --locked --workspace exited0 with236 passes across18 positive
suites/six expected empty suites, zero failures/ignores/filters
(target/existing-sync-workspace-final.log). Host workspace and MSVC three-package
all-target Clippy, excluded native-probe all-target check, actionlint, formatting
and diff checks passed. Independent source/API/test/docs review closed this scope.
Native runtime for this slice remains pending; MSVC compilation is not native
integration evidence. The actual old Windows creation gate stays FAILED. Full
Core resynchronization/publication, CLI/GUI and archive qualification remain
unfinished.

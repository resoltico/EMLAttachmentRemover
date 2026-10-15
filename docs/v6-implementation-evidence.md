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

## Required gates

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

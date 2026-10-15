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
this into a supported product. The workflow remains failed; it does not waive
the required route or junction protection. No NT API fallback was implemented.
The peer QA chat has been asked to review the source, native evidence and
possible safe corrections and reply directly to the implementation chat.

Both jobs observed NTFS through GetVolumeInformationByHandleW, verified actual
native machine through IsWow64Process2, and executed with a restricted effective
token: Administrators deny-only, privileges disabled, membership check false.
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

WP0's native-route challenge remains unresolved pending peer QA. All WP1–WP8
product acceptance gates remain incomplete; the published v5 product/tooling,
artwork, AGENTS.md and release history are preserved while that prerequisite is
resolved. This branch is not a dual-engine product or a delivered v6 purge.

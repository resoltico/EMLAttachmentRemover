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
- `tar -tzvf`: 35 regular files, one relative top-level prefix; no links,
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

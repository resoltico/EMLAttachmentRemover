# Architecture

The workspace separates message policy and native storage from frontends.

- **eml-core** owns request/resource admission, ordered collection, copy naming,
  destination reservations, immutable MIME preparation, independent edit
  authorization, source/existing comparisons, cancellation, publication decisions
  and typed outcomes. Its pure meaning classifier supplies canonical vocabulary,
  conservative outcome/removal meanings and simultaneous consequence guidance
  shared by the adapters. Product display identity also belongs to core. It does not depend on JSON or a GUI framework.
- **eml-fs** binds ordinary files and directories, protects staging, performs
  native no-replace publication and synchronization, and reports independent
  identity/address/cleanup facts. Its reviewed native modules contain the narrow
  unsafe ABI operations; generated platform bindings supply native layouts.
- **remove-eml-attachments** decodes bounded native-path requests, adapts process
  signals and renders checked human or LF-delimited JSON events. Its executable
  calls the reusable business workflow rather than maintaining another engine.
- **eml-protocol** shares native-path/request/event transport across products.
  One strict generic request envelope supports encoding and decoding. Validated
  event facts project into core meaning without a second classifier. Command-line
  arguments, controls and output formatting remain adapter concerns.
- **eml-worker** is the GUI's private process boundary and calls eml-core
  directly. It does not invoke the CLI product. Its separate process preserves
  the required force-stop capability for stalled native operations.
- **eml-app** owns bounded request snapshots, serial queue/report reservations,
  private operation framing, checked protocol reduction, captured run mode,
  terminal run meaning, current activity and report reservations. Processing and
  report export use distinct lanes over one owned child transport. Report drafts
  receive pure lexical validation; the child binds and publishes through native
  no-replace/readback/durability services. It has no UI-toolkit dependency.
  The [frontend design](desktop-frontend-design.md) and
  [process ownership](macos-process-ownership.md) describe its native integration.
- **eml-gui** contains the Slint macOS screens under the explicit macos-gui
  Cargo feature. It renders queue, progress, reports and confirmation state and
  routes native ingress and user actions to the shared application contracts.
  Exact-source GUI qualification is recorded separately. **eml-macos** carries the
  reviewed Darwin ownership boundary into Rust, separate from business policy.
- **xtask** owns repository checks, bounded campaigns, native build identity,
  complete target notices, archive creation/readback and qualification evidence.
- **eml-windows** provides the owned Win32 Job/process boundary used by Windows
  verification tooling. Its narrowly scoped native module keeps unsafe process
  calls out of xtask and filesystem policy. It is not linked into the CLI or
  macOS app and introduces no Windows frontend. See
  [Windows process ownership](windows-process-ownership.md) for its actual scope
  and source-bound native/console evidence.
  Developer process supervision is separate from product helper ownership.

A PreparedMessage owns the single prepared payload and bounded removal metadata.
The forward parser records immutable source spans. Independent authorization
derives a retained-tree proof and iterates its spans in source order; complete
chunked equality, reparsing, structural comparison and gap-free fixed-point
coverage establish that retained bytes and structure remain unchanged. It does not regenerate MIME or treat its own removal list as an oracle.
Real storage observations remain distinct: candidate verification, atomic
publication knowledge, requested address, file/namespace synchronization and
cleanup cannot be collapsed into a Boolean success.

A native release CLI is built once into an owned copy, signed on macOS and frozen.
Its CLI archive consumes those exact bytes. The desktop product builds its own
private worker from the same core/protocol libraries. Archive
verification reads a bounded immutable snapshot, validates exact structure and
legal materials, extracts into an admitted owned directory, checks native runtime
identity and runs the actual extracted executable against independent goldens.
An archive is published into its output directory only after those scoped checks.
This is packaging publication, separate from the product's message-copy edge.

The native target table and tool pins have single authorities under xtask/;
Cargo metadata supplies the workspace version. Generated target legal trees have
reviewed source bindings and deterministic freshness checks. Cargo notices do not
replace native runtime input review. Windows final-link provenance, private
archive extraction/process supervision and macOS app/GUI behavior require their
distinct source-bound native observations. See [verification](verification.md)
for the required checks and their limits.

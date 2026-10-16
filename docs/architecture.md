# Architecture

The workspace separates message policy and native storage from frontends.

- **eml-core** owns request/resource admission, ordered collection, copy naming,
  destination reservations, immutable MIME preparation, independent edit
  authorization, source/existing comparisons, cancellation, publication decisions
  and typed outcomes. It does not depend on JSON or a GUI framework.
- **eml-fs** binds ordinary files and directories, protects staging, performs
  native no-replace publication and synchronization, and reports independent
  identity/address/cleanup facts. Its reviewed native modules contain the narrow
  unsafe ABI operations; generated platform bindings supply native layouts.
- **remove-eml-attachments** decodes bounded native-path requests, adapts process
  signals and renders checked human or LF-delimited JSON events. Its executable
  calls the reusable business workflow rather than maintaining another engine.
- **RunModel** validates protocol 1 bytes and retains bounded results for the app.
  **EMLAttachmentRemover** provides fresh SwiftUI screens and serial helper
  ownership. PlatformIntegration contains only necessary macOS system bridges.
  Helper lifecycle ownership is documented separately in
  [macos-process-ownership](macos-process-ownership.md).
- **xtask** owns repository checks, bounded campaigns, native build identity,
  complete target notices, archive creation/readback and qualification evidence.
  Developer process supervision is separate from product helper ownership.

A PreparedMessage retains source bytes and authorized deletion ranges. Production
verification independently checks that retained content and structure remain
unchanged. It does not regenerate MIME or treat its own removal list as an oracle.
Real storage observations remain distinct: candidate verification, atomic
publication knowledge, requested address, file/namespace synchronization and
cleanup cannot be collapsed into a Boolean success.

A native release CLI is built once into an owned copy, signed on macOS and frozen.
Both its archive and the macOS embedded helper consume those same bytes. Archive
verification reads a bounded immutable snapshot, validates exact structure and
legal materials, extracts into an admitted owned directory, checks native runtime
identity and runs the actual extracted executable against independent goldens.
An archive is published into its output directory only after those scoped checks.
This is packaging publication, separate from the product's message-copy edge.

The native target table and tool pins have single authorities under xtask/;
Cargo metadata supplies the workspace version. Generated target legal trees have
reviewed source bindings and deterministic freshness checks. Cargo notices do not
replace native runtime input review. Windows final-link provenance, private
archive extraction/process supervision and complete app/archive/GUI qualification
remain open; see the [implementation evidence](v6-implementation-evidence.md).

Other-platform GUIs can call the reusable library contracts when an actual
frontend is proposed. There is no second engine, old report decoder, preference
migration, generic GUI scaffold or Python product/tooling dependency.

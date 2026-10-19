# macOS application

The desktop product uses Slint and Rust, with macOS15.0 as its minimum. CLI and
GUI are separate products and thin adapters over eml-core. The GUI's private
eml-worker calls that library directly; it never launches the CLI product.
Windows/Linux GUI implementations are outside the current scope.

Application destination persistence uses only the fresh copyDestinationBytes
String key after an explicit destination action; an argument override remains
session-only. Native system pickers may also retain Cocoa navigation bookmarks
and panel/window geometry. Product code does not read, migrate, reset or write
that OS-managed picker state. The narrow, scenario-specific verification policy
is documented in [verification](verification.md#native-picker-preference-observations).

The explicit Cargo profile is:

```sh
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --locked -p eml-gui --features macos-gui --target aarch64-apple-darwin
```

Use x86_64-apple-darwin on a native Intel Mac. These are separate executables;
there is no universal build. Qualification requires the selected Xcode26.3/17C529
and actual native execution on both CPUs. A newer local SDK build is additional
evidence. Signing is Apple ad hoc only; no Developer ID or notarization is provided.

The initial screen compiles with Slint1.18.1's Winit/software/system-font and
accessibility profile. It explicitly selects both backend and renderer and rejects
the unsupported disabled-shaping override. AuditoryLab's accelerated/software
configuration remains a reference; the initial renderer choice is not an owner
requirement or native graphics qualification. International text, selection,
clipboard, appearance and VoiceOver need actual checks.

eml-app captures immutable native selections/destinations, reserves bounded
reports, frames LF bytes and reduces checked protocol1 observations. eml-protocol
owns wire records shared across products. Display text never reconstructs an
operation path; explicit report export contains accepted structured facts rather
than raw protocol frames or message contents. The technical disclosure also shows
the interface's current native appearance and Increase Contrast observations,
separately from Core outcome/export facts. Native child ownership is described
in [process ownership](macos-process-ownership.md).

After a stream ends without a valid summary, known discovered items without an
accepted result are shown as having no reported result. Complete copies may
exist at their destinations; the app does not label those missing observations
as created or failed or invent a summary. An accepted Core summary remains
authoritative, including its explicit not_processed count. Earlier accepted
copy and publication receipts remain available when a later stream fails.

The controller connects message/folder/destination sheets, immutable queued runs,
retained reports, cooperative Stop, confirmed Force Stop and Quit, and explicit
protected report export. Export captures the exact run's accepted facts and
terminal worker lifecycle. Its cancellation has separate intent and never rolls
back attempted publication; its reservation lasts through task/buffer disposal
and join. Received results and removed-part details are paged in bounded views.
The event loop remains running when windows are hidden. Minimal application
hooks extend only absent lifecycle methods on Winit's verified delegate; native
Quit is deferred to the controller and reopen restores the main window. Actual
native GUI lifecycle acceptance remains required.

The pinned Winit source is retained under vendor/winit. Its focused macOS patch
passes one synchronous, borrowed native pasteboard to the common bounded
file-URL reader. Inspection never queues work; a committed drop validates the
current complete batch again. The old filename-string/per-file drop path is
removed. See the retained [patch notice](../vendor/winit/EML-PATCH.md) for upstream
identity, changed files and the separate native drag-delivery requirement.
Read-only report text supports Edit/Copy and Select All through Slint; Window
commands use Slint's minimize, maximize and macOS bring-to-front APIs.
Required behavior includes file/folder selection, native URL batch ingress,
Services/drop/Open With, destination preferences with session-only override,
serial queued runs, two retained reports, Stop/Force Stop, close/hide/reopen,
confirmed bounded Quit, exact Finder reveal and export/failure handling.
The bundle declares both EML messages and folders for native URL delivery;
both declarations retain `LSHandlerRank=None`. It does not set default file or
folder associations. The separate Services declaration accepts the same types.
Local
development-window observations cover message selection, a real core-backed run
with independent golden/original checks, explicit report save, metadata and
technical disclosure, supplied SVG artwork, and idle Quit confirmation/cancel.
Those observations do not qualify pinned/native2 or the remaining system/GUI
paths. `cargo xtask build-macos-sources` currently
retains native Rust release executable/profile/ad-hoc-signature evidence and
succeeds when those requested outputs validate. Complete bundle/legal/archive/GUI
qualification remains a separate open gate.

The complete builder is `cargo xtask build-macos-app --icons ICON_PRODUCER_DIR
--out NEW_OUTPUT_DIR`. It consumes same-source validated icon and component legal
inputs, preserves signed worker bytes when signing the outer app, verifies every
Mach-O's CPU/minimum/system dependencies, and uses actual ditto creation/extraction.
The frozen producer reference checks file identities and modes before extraction;
actual extracted worker runs check Core outcomes against independent goldens and
unchanged originals. Hosted startup smoke observes the exact owned GUI PID's real
window and settles that process without sending requests. This is not full GUI,
paint, accessibility or UI Quit acceptance. Native execution at the delivered head
remains required before reporting these gates passed.

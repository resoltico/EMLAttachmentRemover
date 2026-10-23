# macOS application

The desktop product uses Slint and Rust, with macOS15.0 as its minimum. CLI and
GUI are separate products and thin adapters over eml-core. The GUI's private
eml-worker calls that library directly; it never launches the CLI product.
Windows/Linux GUI implementations are outside the current scope.

The standard native About panel is available from **EML Attachment Remover →
About EML Attachment Remover**. Its bundle metadata and Credits contain the product
name, release version, distinct build number, copyright and license information.
**Help → About Slint and Licenses…** displays the actual `AboutSlint` attribution
widget under the selected royalty-free license, plus complete software/artwork
license text. Its Third-party licenses action opens bundled notices and license
files in Finder. **Help → Project Website** opens the project website.

Application destination persistence uses only the copyDestinationBytes
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
there is no universal build. Qualification requires the selected Xcode version/build in `xtask/tools.toml`
and actual native execution on both CPUs. A newer local SDK build is additional
evidence. Signing is Apple ad hoc only; no Developer ID or notarization is provided.

The screen uses the pinned Slint Winit/software/system-font and accessibility
profile. Both application windows explicitly select the registered macOS
system UI family discovered through Fontique, rather than relying on Slint's
generic sans-serif fallback. Their base text size comes from AppKit's
`NSFont.systemFontSize`; headings retain their explicit display sizes. The Fluent
widget style is separate from this native typography selection. Optical sizing and
tracking use logical points consistently for shaping, metrics and software drawing
at either display density; see the [vendored typography patches](../vendor/typography.md).
The app explicitly selects both backend and renderer and rejects the
unsupported disabled-shaping override. International text, selection, clipboard,
appearance and VoiceOver require actual native checks.

eml-app captures immutable native selections, modes and destinations, reserves bounded
reports, owns processing/export children and reduces checked protocol1 observations.
The private desktop transport is protocol2; the public CLI transport remains protocol1. eml-protocol
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
protected report export. Export captures the exact run's accepted facts and terminal
worker lifecycle. Choose an existing folder, then enter a filename in the export
form; the pure application draft validates the exact destination. Existing files
are never replaced. Occupancy is a worker refusal that leaves the draft editable;
the GUI performs no target-filesystem occupancy check. The export child owns all
publication I/O. Its source-report reservation lasts through actual child, pipe,
reader/writer and process-owner settlement. Processing Stop/Force Stop and export
Cancel Export/Force Stop Export carry distinct operation identities. Forced export
termination can leave an unknown outcome and temporary artifacts when cleanup
could not be observed. A qualified receipt followed by a transport/process failure
retains both facts.

Choose **Check only** or **Create copies** for later selections; Create copies is
the default. Check shows the exact core-planned destinations without creating
copies. **Create from this check** enqueues the immutable captured selection with
fresh IDs and Create mode. Filesystem contents can change after a check; all
collection, MIME, identity, destination and publication checks run again. A check
grants no later Create authority. Existing report-capacity and explicit-dismissal
rules apply.

Received results and removed-part details are paged in bounded views. Byte comparisons
and proposed removals describe the prepared copy; they do not claim a copy was
published or disk space was freed. Run-level errors remain visible when no message
results exist, with Core guidance separate from the precise diagnostic.
The event loop remains running when windows are hidden. Source-defined typed lifecycle callbacks on the retained Winit delegate defer
native Quit to application confirmation and restore the main window on reopen.
Services uses a separate retained application-owned provider. Control intents drain
independently of diagnostics, after any reentrant native batch collector unwinds.
Confirmed Quit stops both child lanes and waits for actual settlement; automatic
escalation uses the one confirmation instant and does not equate timeout with exit. Actual
native GUI lifecycle acceptance remains required.

The pinned Winit source is retained under vendor/winit. Its focused macOS patch
passes synchronous lifetime-bound native open-URL batches and one borrowed pasteboard
to the common bounded file-URL reader. Inspection never queues work; a committed drop validates the
current complete batch again. See the retained [patch notice](../vendor/winit/EML-PATCH.md) for upstream
identity, changed files and the separate native drag-delivery requirement.
Read-only report text supports Edit/Copy and Select All through Slint. While a
native file or folder dialog is active, Copy, Paste (Command-V) and Select All
use AppKit's current responder. Window commands use Slint's minimize, maximize
and macOS bring-to-front APIs.
Required behavior includes file/folder selection, native URL batch ingress,
Services/drop/Open With, destination preferences with session-only override,
serial queued runs, two retained reports, Stop/Force Stop, close/hide/reopen,
confirmed bounded Quit, exact Finder reveal and export/failure handling.
The bundle declares both EML messages and folders for native URL delivery;
both declarations retain `LSHandlerRank=None`. It does not set default file or
folder associations. The separate Services declaration accepts the same types.
`cargo xtask build-macos-sources` retains native Rust executable/profile and
ad-hoc-signature evidence. Complete bundle/legal/archive/GUI acceptance requires
the builder and separate consumers below; a source build or startup-window
observation cannot qualify those integrations.

The complete builder is `cargo xtask build-macos-app --icons ICON_PRODUCER_DIR
--out NEW_OUTPUT_DIR`. It consumes same-source validated icon and component legal
inputs, preserves signed worker bytes when signing the outer app, verifies every
Mach-O's CPU/minimum/system dependencies, and uses actual ditto creation/extraction.
The frozen producer reference checks file identities and modes before extraction;
actual extracted worker runs check Core outcomes against independent goldens and
unchanged originals. Hosted startup smoke observes the exact owned GUI PID's real
window and settles that process without sending requests. This is not full GUI,
paint, accessibility or UI Quit acceptance. Startup failure evidence includes
owned-PID window observations and retained GUI stdout/stderr. While the owned GUI
remains running, the harness attempts a short managed process sample before
cleanup; diagnostic failures remain separate from the original observation failure.
Native execution at the delivered head
remains required before reporting these gates passed.

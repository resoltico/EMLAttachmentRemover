# Slint Qt source maintenance patch

This is the published `i-slint-backend-qt` 1.18.1 source package, retained under
its upstream licenses in `LICENSES/`. Slint remains version 1.18.1. The Qt
backend retains the upstream QAccessible accessibility implementation and
supports Qt's native Wayland and X11 platforms.

Source: <https://crates.io/crates/i-slint-backend-qt/1.18.1>

- Published `.crate` SHA-256:
  `a32cd6edc5422d1525fbe4ed29635b642fdbb26ae7036abc20eee063faa45d4a`
- Upstream commit: `372cf0ee5577c3dfec309a45e7b778ba4e81b734`
- Upstream path: `internal/backends/qt`

The package is excluded from the application workspace and selected with a
Cargo source patch. Cargo publication artifacts and the package lockfile are
omitted. The normalized manifest retains exact Slint dependencies, disables
publication and adds `url` for native filesystem URL decoding.

Local changes:

1. Drop events transport fully encoded file URLs instead of converting through
   QString filesystem paths. Decoding through `url::Url::to_file_path` preserves
   Unix bytes and rejects a mixed local/remote list as a whole.
2. The `raw-window-handle-06` feature exposes native Xlib window/display and
   Wayland surface/display handles through Slint's existing adapter API. This
   enables native portal dialogs without using Qt's nonnative chooser, whose
   QString filesystem paths lose non-UTF8 bytes. Qt's shipped QPA header is a
   private interface: the window factory rejects a runtime Qt version different
   from the SDK version before our private virtual calls. The native display
   connection and widget remain owned by Qt while the borrowed adapter exists.
   Hidden Wayland widgets return an unavailable handle: without an xdg_toplevel
   role, attempting to export them would cause a compositor protocol error.

3. The enabled backend build fails when Qt is absent, unsupported, older than
   Qt 6.12, outside the Qt 6 series, explicitly
   disabled by `SLINT_NO_QT`, or built for WebAssembly. It cannot silently
   produce a nonfunctional native desktop backend.

4. The native accessibility bridge maps Slint's enabled, selectable, selected
   and visibility facts into Qt state flags and observes their changes. Item
   visibility also requires its native QWidget to be visible; window flags
   derive visibility, enabled state, active state and focus from that QWidget.
   Static text is not advertised as focusable; interactive roles retain focusability.
   Valid enabled focusable items expose Qt's focus action, forwarding to the
   same selection-preserving Slint focus path used by the Winit bridge.
   Focus is appended after existing activation actions, preserving default Press.
   State notifications carry only actual changed flags. Declared live-label
   changes keep NameChanged events and emit Qt announcements with the declared
   politeness, excluding hidden, empty and unchanged text. The initial tree
   does not announce every startup label. Later inserted live subtrees queue
   their initial visible text after the parent caches commit, sharing the same
   label cache with property notifications to avoid duplicate announcements.
   Existing checked, expandable, expanded, read-only and focus state meanings
   are retained.

5. Actual owned text inputs expose Qt's TextInterface, including read-only
   licenses, diagnostics and selectable About text. `qt_accessible/text.rs`
   resolves weak items on each call and uses the existing Slint text model,
   Unicode offset conversion, renderer hit testing and caret geometry. Qt
   grapheme boundaries produce character boxes; out-of-viewport hit tests fail.
   Selection/cursor actions use the existing core selection path. Bounded
   Flickable scrolling reveals the requested text without changing selection
   or cursor. Text, selection and caret changes have their native Qt events.
   No EditableTextInterface is advertised, password models are excluded, and
   text is not misrepresented as a numeric ValueInterface.

6. Embedded popup roots are attached to the same accessible invocation parent
   as the Winit backend, using temporary child snapshots and native-window
   coordinate mapping. Dismissed popup nodes keep only weak model references.
   Descendant queries upgrade the actual ItemWeak instead of reinterpreting it
   as an ItemRc. Each native window owns and frees one refreshable weak root
   slot, replacing the upstream allocation on every root lookup. The window
   pointer is an owned raw Weak, so getters borrow/upgrade it before reading
   QtWindow data, including during construction or deferred native deletion.
   Each temporary child ItemWeak box is also freed after the Qt item has cloned
   its weak ownership, for both new and reused children.
   Hit testing follows the active popup paint stack before the base scene,
   recurses through overflow descendants and intersects actual ancestor clips.
   Nodes removed from the current tree are detached before deferred QObject
   deletion; queued callbacks and actions cannot reuse retired nodes.

Tests in `qt_accessible.rs` exercise the production pure Qt state mapping,
state-change masks and announcement conditions; text helper tests cover independent
UTF-16/UTF-8 and scrolling oracles, without an OS accessibility
service. Native AT-SPI tests remain external field evidence.

Tests in `file_transfer.rs` exercise URL rejection, empty transfer results,
non-UTF8 bytes and newline filenames. The Qt conversion test includes the old
lossy conversion as a negative control. The Linux `cargo xtask ci` profile explicitly runs these library tests together
with the application's portal boundary tests, selecting the supported native Qt
profile with `raw-window-handle-06`.

Qt's nonnative filesystem chooser was tested and shown to replace non-UTF8
Unix bytes before returning a URL. It is deliberately not used. Native portal
selection and parent-token integration are verified separately from these pure
URL-boundary tests. Qt uses its QPainter renderer rather than Slint's software
or Skia renderer.

Retire this patch when a maintained upstream Slint release provides the required
native byte handling, raw window handles and required-backend failure
semantics. Do not remove the protections merely to remove the patch.

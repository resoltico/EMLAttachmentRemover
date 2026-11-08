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

3. The enabled backend build fails when Qt is absent, unsupported, older than Qt 6.12, outside the Qt 6 series, explicitly
   disabled by `SLINT_NO_QT`, or built for WebAssembly. It cannot silently
   produce a nonfunctional native desktop backend.

Tests in `file_transfer.rs` exercise URL rejection, empty transfer results,
non-UTF8 bytes and newline filenames. The Qt conversion test includes the old
lossy conversion as a negative control. The Linux `cargo xtask ci` profile explicitly runs these library tests with
`raw-window-handle-06`, alongside the application's portal boundary tests.

Qt's nonnative filesystem chooser was tested and shown to replace non-UTF8
Unix bytes before returning a URL. It is deliberately not used. Native portal
selection and parent-token integration are verified separately from these pure
URL-boundary tests. Qt uses its QPainter renderer rather than Slint's software
or Skia renderer.

Retire this patch when a maintained upstream Slint release provides the required
native byte handling, raw window handles and required-backend failure
semantics. Do not remove the protections merely to remove the patch.

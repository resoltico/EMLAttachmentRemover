# Native file-URL batch bridge

This directory retains the published winit0.30.13 crate, including its Apache2.0
license and notices. Upstream revision: e9809ef54b18499bb4f2cac945719ecc2a61061b.
Published archive SHA256:
a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d.
The hash establishes the input's identity, not review quality.

The two modified upstream files are src/platform/macos.rs and
src/platform_impl/macos/window_delegate.rs. They add one optional, synchronous
macOS callback for a retained native pasteboard, with inspection and commit
phases. The application validates and admits the whole file-URL batch through
the same bounded receiver as Services. Winit neither interprets operational
filenames nor constructs per-file events. Only the Copy source operation is
accepted. The draggingEntered: method uses the native NSDragOperation return
type. Windows and Linux window implementations are unchanged.

Design challenge: Winit's original per-file events have no native batch delimiter
and convert NSString filenames through text. Timer grouping cannot recover the
batch or original bytes. A delegate replacement or runtime method replacement
would hide more ownership and ABI assumptions. The narrow source patch keeps the
actual registered native delegate and exposes only a borrowed object for the
duration of an OS callback. Inspection does not enqueue; commit reads and
validates the current complete payload again. Panics cannot cross the native
callback. No compatibility decoder or second ingress path is retained.

This patch requires real native drag integration qualification on both Mac CPUs;
compilation and an owned pasteboard test alone do not establish drag delivery.

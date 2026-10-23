# Native file-URL batches and application lifecycle

This directory retains the published winit0.30.13 crate, including its Apache2.0
license and notices. Upstream revision: e9809ef54b18499bb4f2cac945719ecc2a61061b.
Published archive SHA256:
a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d.
The hash establishes the input's identity, not review quality.

Modified upstream files:

- `src/platform/macos.rs`: optional typed application callback registration,
  lifetime-bound URL array and existing drag receiver interface.
- `src/platform_impl/macos/event_loop.rs`: carry registration into delegate creation.
- `src/platform_impl/macos/app_state.rs`: source-declared Quit, reopen, close-last
  and open-URL delegate methods; synchronous adapter invocation with unwind containment.
- `src/platform_impl/macos/window_delegate.rs`: retained atomic pasteboard drag bridge.

Winit retains its delegate and event-loop ownership. The callback is copied before
invocation; no mutable internal borrow survives native adapter reentrancy.
`BorrowedFileUrls<'a>` has a fork-private constructor and is neither Send nor Sync.
Its retained array owner outlives the synchronous callback; safe Rust cannot retain
that value beyond the callback lifetime. Compile-fail examples check these bounds.
One explicitly documented native pointer conversion bridges the application's
objc2 version; the pointer must not escape or leave the main thread. Application
code reserves bounded ingress before conversion, checks actual NSURL types, and
commits only a fully validated owned batch. Quit always cancels native termination
while application confirmation/settlement runs; reopen and panic return conservative
native results. Callback and diagnostic-callback panics cannot cross AppKit.
Services is a separate application-owned retained provider, not a delegate mutation.

The drag bridge passes one retained native pasteboard with inspection and commit
phases. The application validates the complete file-URL batch through the bounded
receiver shared with Services/Open With. Winit does not interpret names or construct
per-file events. Only Copy is accepted, and draggingEntered: uses NSDragOperation.
Windows and Linux implementations are unchanged.

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

Native qualification must execute actual packaged startup Open With, Dock/window
drop, Services, callback reentrancy/panic, Quit cancellation and confirmed settlement,
close-last-window/reopen, standard About, Help/About Slint and Licenses, keyboard
access and accessibility on both native macOS CPUs. Compilation, compile-fail
checks and pasteboard tests do not establish these native delivery properties.

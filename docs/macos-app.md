# macOS application

The desktop product uses Slint and Rust, with macOS15.0 as its minimum. CLI and
GUI are separate products and thin adapters over eml-core. The GUI's private
eml-worker calls that library directly; it never launches the CLI product.
Windows/Linux GUI implementations are outside the current scope.

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
than raw protocol frames or message contents. Native child ownership is described
in [process ownership](macos-process-ownership.md).

Current controls are inactive while supervisor and system integration are wired.
Required behavior includes file/folder selection, native URL batch ingress,
Services/drop/Open With, destination preferences with session-only override,
serial queued runs, two retained reports, Stop/Force Stop, close/hide/reopen,
confirmed bounded Quit, exact Finder reveal and export/failure handling. None is
qualified by the initial screen build. `cargo xtask build-macos-app` currently
retains native Rust release executable/profile/ad-hoc-signature evidence and
fails explicitly because complete bundle/legal/archive/GUI gates remain open.

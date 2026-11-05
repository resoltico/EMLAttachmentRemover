# spin_on source maintenance patch

This workspace patch replaces the current `spin_on` 0.1.1 dependency of Slint
1.18.1. Upstream uses `pin-utils::pin_mut!`; `pin-utils` is archived and is covered
by [RUSTSEC-2026-0441](https://rustsec.org/advisories/RUSTSEC-2026-0441.html).
No published `spin_on` or Slint update removes that dependency as of 2026-11-05.

The implementation uses `core::pin::pin!`, `Waker::noop()` and
`core::hint::spin_loop()`, available under the workspace's Rust toolchain.
It retains the current generic future API, `no_std`, busy polling and completion
behavior while removing the dependency and the hand-written unsafe raw waker.
Tests exercise immediate completion and a non-Unpin future that remains pinned
through pending polls, invokes its waker and is dropped exactly once.

The Apache-2.0/MIT license files and upstream authorship are retained from the
[0.1.1 source package](https://crates.io/crates/spin_on/0.1.1).
Remove the workspace patch when upstream consumers accept a maintained release
that removes `pin-utils`. The advisory policy remains unchanged.

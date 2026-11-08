# Slint menu identity source maintenance patch

Published `i-slint-core` 1.18.1, under its original licenses in `LICENSES/`.
Source: <https://crates.io/crates/i-slint-core/1.18.1>.
Published `.crate` SHA-256: `ed2ea15059d70bc31b1a8b1eb4c34f2c5df04cb7e53f36179b43c0261231d646`.
Slint remains version 1.18.1. The normalized dependency manifest is retained,
with publication disabled and its reference to the omitted `Cargo.toml.orig`
removed. Publication markers, the original workspace manifest and package
lockfile are omitted.

The only Rust change is in `menus.rs`. The published menu producer clears its
entry-ID cache whenever any menu property changes. A displayed fallback popup
still holds its earlier entries, so a background state change can make an
unchanged, enabled command silently stop dispatching. This was reproduced in the
actual Linux GUI with File → Close Window across Removing → Done, as well as
Edit → Copy when opening the menu changes physical editor focus.

Rebuilds now preserve an ID only for the same actual `ItemWeak` in the current
menu tree. Removed entries disappear from the current cache; reappearing items
receive fresh monotonically allocated IDs. There are no old-ID aliases or
label/index matching. Activation refreshes current membership and checks the
live enabled property before changing checked state or invoking the command.
It resolves an owned item and releases the cache borrow before evaluating
bindings or invoking callbacks, allowing a callback to query/rebuild its menu.

`crates/eml-gui/tests/menu_contract.rs` exercises the public runtime menu producer
with actual menu items and a conditional tree visitor. It independently checks
stable identity, disabled checkable commands, retired IDs and reentrant queries.
The GUI acceptance tests and native field replay exercise the application's
actual generated menus. Original upstream library tests are not claimed: the
published package references a font fixture outside its packaged source tree.

Retire this patch when the maintained pinned toolkit supplies these menu
identity and activation guarantees. Preserve current-state validation and retired
entry rejection when doing so. This patch changes no email processing rules,
protocol serialization or toolkit version.

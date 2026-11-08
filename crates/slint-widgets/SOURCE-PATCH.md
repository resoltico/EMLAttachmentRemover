# Slint Fluent widget source maintenance patch

This is the complete published Fluent import/image closure from
`i-slint-compiler` 1.18.1, under the original licenses in `LICENSES/`.
Source: <https://crates.io/crates/i-slint-compiler/1.18.1>.
Published `.crate` SHA-256: `f55d48ebef05e2bf09ba5a1fc67bcb11033b9e12a2a50ed3105c7d25e2b539d0`.

The original closure contains 66 files, 233970 bytes and
5588 Slint source lines. `fluent/` is named `eml-fluent/` so the application
selects this source provider explicitly. Relative imports and every visual/layout
property remain unchanged. `UPSTREAM-SOURCES.sha256` records the original bytes
for each included source/image under its vendored path. Three files differ:
`common/menu-base.slint`, `common/menus.slint` and `eml-fluent/menu.slint`.

The menu base's focus scopes inside its actual interactive areas expose button
roles, entry-provided labels, enabled state and default actions calling the
existing `clicked`/`activate` callbacks. Checkable entries forward their existing
checked facts and shortcuts are exposed as descriptions. Disabled entries and separators refuse activation.
Caption and shortcut children are presentation-only, avoiding duplicate names.
The current entry receives actual focus through the existing keyboard/hover
selection path; the Fluent wrapper forwards that focus call. Focus scopes expand
to their parent geometry and key events retain the existing menu handler.
No menu roles unsupported by Slint 1.18.1, expanded state, commands or business
rules are invented. The existing menu command tree, keyboard handling and visual
style stay authoritative.

Linux uses the supported compiler style/include-path APIs to load this bundle.
It needs the broad experimental compiler switch because the published widget
sources already use internal interfaces and properties. The application does not
use new experimental UI constructs. macOS uses unmodified builtin Cupertino and
its native menu bar. The small `slint-build-source` patch supplies a typed setter
without changing global environment or forking the compiler.

Retire this bundle when the maintained pinned toolkit exposes actionable,
correctly labelled fallback menu items. Preserve disabled/checkable semantics and
native macOS menus when doing so.

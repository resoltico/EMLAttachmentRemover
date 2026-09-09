# macOS Finder and Shortcuts

Install the bundled launcher with:

```sh
/bin/sh integrations/macos-shortcuts/install.sh
```

The installer uses a private application-support directory. If a v2 bundle exists,
run its uninstaller, delete the old Shortcut manually, then install v3; v3 never
migrates a v2 bundle in place.

Create a Finder Quick Action in **Shortcuts** named **Create MIME-Pruned EML Copy**.
Configure it to receive **Files** from Finder, add **Run Shell Script**, set input to
**Shortcut Input**, select **as arguments**, and use:

```sh
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

The launcher defaults to `EML_REMOVER_EXISTING=verify`, so repeat use accepts only a
destination whose exact bytes equal the current verified candidate. It also accepts
`error`; removed values `skip` and `force` produce migration diagnostics. It forwards
HUP/INT/TERM to its child, waits for cleanup/reporting, validates schema 3, and reveals
only `created` or `existing_verified` paths whose address receipt succeeded.

Retained HTML is not sanitized. A related component can be removed while retained HTML
still references it; Apple Mail may show a missing-resource placeholder. This is an
expected MIME-pruning boundary, not a claim that appearance is unchanged.

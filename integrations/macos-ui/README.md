# Native macOS report window

Open the app and choose files, use File → Open EML Files, or invoke the existing Finder Quick Action. Cancelling the file picker does not interrupt another batch.

The native application presents processing progress and final results for Finder Quick Actions. It bundles the same zipapp and validated launcher used by Terminal; it does not parse human output or implement another MIME processor. The window shows the actual bundled processor version.

## Install a prebuilt release

Download `eml_attachment_remover-VERSION-macos-universal.zip` from the official GitHub Release and extract it. This prebuilt universal app requires macOS 14 or later and CPython 3.14; Swift, Xcode, and Apple Command Line Tools are not installation prerequisites. Install CPython 3.14 first, quit an older app before updating, and run the instructions in the archive's `INSTALL.txt` to record your interpreter and install into `~/Applications`. You can also copy the app there and rely on runtime discovery; that does not configure the optional Terminal launcher.

The release app is already ad-hoc signed by CI. Customers do not re-sign it. It has no Developer ID and is not notarized, so Gatekeeper may block an internet download. If you trust the official download, follow [Apple's app-specific Open Anyway instructions](https://support.apple.com/102445); device-management policy may prohibit an exception. Signature integrity and Gatekeeper approval are separate checks. Do not disable Gatekeeper or strip quarantine to install this application.

## Build from source

Building requires macOS, Apple Command Line Tools with Swift 6, and CPython 3.14. The app contains Apple Silicon and Intel executables targeting macOS 14 or later. It still requires CPython 3.14 at runtime; the native UI does not bundle Python. Only the host architecture and OS used for qualification are live-tested locally.

Use a fresh destination for every build. The build reads the current working tree, creates a fresh zipapp, compiles both native architectures, and ad-hoc signs the complete bundle. The same signing command runs in GitHub macOS CI; it requires no developer certificate or secret. Ad-hoc signing is not Developer ID signing or notarization.

```sh
/bin/sh integrations/macos-ui/build.sh "$HOME/Downloads/EML Attachment Remover.app"
/bin/sh integrations/macos-ui/install.sh "$HOME/Downloads/EML Attachment Remover.app"
```

Quit the application before updating it; the installer refuses to replace a running installed bundle. The installer records the selected CPython interpreter in a private runtime configuration outside the app, so Finder launches do not depend on shell environment inheritance. Reinstall with `EML_REMOVER_PYTHON` to change that selection.

The installer defaults to `~/Applications/EML Attachment Remover.app`, refuses an unowned or unmarked existing bundle, and retains the previous application in a private application-support backup directory. It also updates the direct Terminal launcher and zipapp. `EML_REMOVER_PYTHON` selects the build/install interpreter; `EML_REMOVER_UI_APP` selects an alternative app installation path.

## Finder Quick Action

Keep the existing shortcut name if other workflows invoke it. Configure it to receive Files from Finder and use one Run Shell Script action with Shortcut Input passed **as arguments**:

```sh
/usr/bin/open -a "$HOME/Applications/EML Attachment Remover.app" -- "$@"
```

Remove the old Show Content action. The application accepts Finder file-open events and creates a separate processing/report window for each batch; an already open report does not hold the Quick Action open. The shortcut finishes when macOS accepts the application launch, not when processing completes. Automation requiring processing results or exit status must use the direct launcher or zipapp instead.

The running window offers **Stop processing**. This forwards interruption through the existing launcher and preserves completed item receipts; it does not undo copies. The close control is unavailable while that window's run is active. A private lifetime pipe triggers launcher cancellation if the app disappears, including forced termination. Quitting the application stops its active batches and waits for the launcher to finish. A final report uses **Done**, which closes the report without changing files.

Final reports distinguish created copies, verified existing copies, failures, unprocessed or stopped files, and copies published with an error. Invocation interruption and output-finalization failure remain visible even when item receipts succeeded. Invalid or incomplete reports never imply that no files were created. Finder reveal of a copy is offered only for an accepted verified address during an ordinary completed invocation; other reports can show the source folder.

Details show file outcomes, warning and error codes, and paths; Copy details retains the complete admitted receipt and exact native path evidence. Large detail displays are bounded with an explicit truncation notice; **Copy details** copies the complete report. File lists are virtualized, the content scrolls, and final controls remain accessible below the scrolling content. The application does not upload reports or EML contents.

## Artwork

All custom graphics are drawn from original project geometry by `Artwork.swift`; no system symbol images or third-party icon library are used. The same renderer produces the application icon and interface graphics. See [artwork provenance and licensing](ARTWORK.md). The native operating-system controls and fonts remain runtime platform components.

## Qualification

```sh
/bin/sh integrations/macos-ui/test.sh
```

The normal macOS pytest lanes also compile and test the presentation model, build and verify a fresh signed universal bundle, and process a synthetic EML through its bundled launcher. GUI lifecycle, cancellation, Finder integration, appearance, and keyboard checks require live macOS qualification.

## Release production

Maintainers use `uv run python -B -m tools.release_delivery --output-directory /absolute/fresh/output` on macOS. The same tool reverifies downloaded CI artifacts with `--verify-directory`. See the [release pipeline and audit contract](RELEASE.md), including the exact asset set, archive checks, signature checks, and publication boundary. Portable-only CLI qualification remains available through `tools/qualify_release.py`; it is not accepted as a complete v4 publication.

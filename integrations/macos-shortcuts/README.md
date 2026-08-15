# macOS Finder and Shortcuts integration

The launcher in this directory is deliberately separate from the portable MIME
processor. The processor remains operating-system neutral; this wrapper only handles
Finder input, Python discovery, Shortcuts-friendly reporting, and revealing the
created files in Finder.

## Install the launcher

From Terminal, inside the extracted project directory:

```sh
/bin/sh integrations/macos-shortcuts/install.sh
```

This copies the zipapp and launcher, plus a small installation-ownership marker, to:

```text
~/Library/Application Support/EML Attachment Remover/
```

No administrator rights are required. The project directory may be moved or deleted
after installation, but retain it (or reacquire the same release) if you later need its
uninstaller. A custom `EML_REMOVER_HOME` must resolve to a dedicated directory: the
installer refuses the filesystem root, your home directory or one of its ancestors, an
existing unmarked directory, symbolic-link destinations, managed names containing
anything other than regular files, and unknown directory entries. The dedicated
installation directory is owner-only (`0700`), and its ownership marker is published
only after both payloads are safely in place. The installer filesystem support library
is a public implementation file used from the extracted project; it is not copied into
the finished installation.

By default, every installer run rebuilds a verified local zipapp from the current
public source with `python3.14` before it copies the launcher; a stale existing default
build is never reused. If that command has a different name or path, set
`EML_REMOVER_PYTHON`; to install a separately downloaded, verified official archive,
set `EML_REMOVER_ZIPAPP` instead:

```sh
EML_REMOVER_PYTHON="/absolute/path/to/python3.14" \
/bin/sh integrations/macos-shortcuts/install.sh

EML_REMOVER_ZIPAPP="$HOME/Downloads/remove-eml-attachments.pyz" \
/bin/sh integrations/macos-shortcuts/install.sh
```

The selected zipapp must itself be a direct regular file, not a symbolic link or other
special filesystem entry. The installer prints the exact shell command for the final,
canonical installation directory. Use that printed command when you configure
Shortcuts, especially after selecting a custom `EML_REMOVER_HOME`; it safely quotes the
custom path and passes the same directory to the launcher.

## Create the Shortcut

1. Open **Shortcuts** and create a new shortcut.
2. Name it **Remove EML Attachments**.
3. Open the shortcut details and enable **Use as Quick Action**.
4. Enable **Finder** and configure the shortcut to receive **Files**.
5. Add **Run Shell Script**.
6. Select `/bin/zsh` or `/bin/sh` as the shell.
7. Set the action's **Input** to **Shortcut Input**. An empty or generic **Input**
   variable does not pass Finder's selected files.
8. Set **Pass Input** to **as arguments**.
9. Paste the command printed by the installer. For the default installation, this
   shorter command is equivalent:

```sh
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

The Quick Action then appears in Finder's context menu. It accepts multiple selected
EML files in one invocation and creates each output beside its source. By default it
asks Finder to reveal each created or retained output; after a batch, Finder normally
leaves the last output selected.

Before relying on it, close and reopen the shortcut once and confirm that it still
shows **Receive Files**, **Input: Shortcut Input**, and **Pass Input: as arguments**.
This catches an incompletely committed Shortcuts edit that would otherwise produce an
inert Finder action.

## Existing outputs

The safe default is to refuse to replace an existing `*.attachments-removed.eml` file. The
wrapper recognises an optional environment variable:

```text
EML_REMOVER_EXISTING=error   refuse replacement; default
EML_REMOVER_EXISTING=skip    retain and report existing outputs
EML_REMOVER_EXISTING=force   replace existing outputs, never a selected source
```

The environment can be set in the Shortcuts shell action before the launcher call:

```sh
EML_REMOVER_EXISTING=skip \
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

## Python discovery

The launcher checks the usual Homebrew, python.org, user-local, and pyenv locations
for Python 3.14. To select an interpreter explicitly:

```sh
EML_REMOVER_PYTHON="/absolute/path/to/python3.14" \
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

## Finder reveal

The launcher asks Finder to reveal each output by default. Reveal is best-effort: a
Finder launch error or timeout does not turn a successfully processed EML into a
failure. Disable reveal with:

```sh
EML_REMOVER_REVEAL=0 \
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

## macOS permissions

If Shortcuts reports that it cannot read or write a selected location, grant the
Shortcuts application access under **System Settings → Privacy & Security → Files
and Folders**. Full Disk Access is normally unnecessary unless the selected files are
inside a protected location.

## Uninstall

The payload uninstaller cannot delete a shortcut from the Shortcuts library. Remove
both pieces explicitly:

1. In **Shortcuts**, find **Remove EML Attachments**, choose **Delete**, and confirm the
   deletion. If Shortcuts uses iCloud, that confirmation also removes this shortcut
   from your other synced devices.
2. From the retained project directory, or the same release extracted again, run:

```sh
/bin/sh integrations/macos-shortcuts/uninstall.sh
```

For a custom installation, set the same directory used during installation:

```sh
EML_REMOVER_HOME="/absolute/dedicated/path" \
/bin/sh integrations/macos-shortcuts/uninstall.sh
```

The uninstaller requires the exact ownership-marker bytes and preflights every directory
entry before removing anything. It removes only the two installed regular files and
then that marker, and never uses recursive removal. If the directory contains any
unknown entry or an unsafe managed entry, everything remains in place for manual
inspection. If a later removal step fails, the ownership marker remains or is restored.

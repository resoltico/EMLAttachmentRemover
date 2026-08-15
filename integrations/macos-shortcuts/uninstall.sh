#!/bin/sh

set -eu

fail() {
    printf '%s\n' "$2" >&2
    exit "$1"
}

canonical_directory() {
    RESOLVED_DIRECTORY=$(
        CDPATH='' cd -P "$1" 2>/dev/null && {
            pwd -P
            printf '\037'
        }
    ) || return 1
    RESOLVED_DIRECTORY=${RESOLVED_DIRECTORY%?}
    RESOLVED_DIRECTORY=${RESOLVED_DIRECTORY%?}
}

entry_exists() {
    [ -e "$1" ] || [ -L "$1" ]
}

optional_regular_file() {
    ! entry_exists "$1" || { [ -f "$1" ] && [ ! -L "$1" ]; }
}

marker_is_exact() {
    [ -f "$1" ] && [ ! -L "$1" ] \
        && [ "$(LC_ALL=C wc -c <"$1")" -eq "$MARKER_BYTES" ] \
        && grep -F -x -q "$MARKER_TEXT" "$1"
}

has_unknown_entry() {
    for entry in \
        ./* \
        ./.[!.]* \
        ./..?*
    do
        entry_exists "$entry" || continue
        case "$entry" in
            "./$MARKER_NAME"|\
            ./remove-eml-attachments.pyz|\
            ./run-from-finder.sh)
                ;;
            *)
                return 0
                ;;
        esac
    done
    return 1
}

restore_marker() {
    restore_temp=$(mktemp ./.marker.restore.XXXXXX) \
        || return 1
    if ! printf '%s\n' "$MARKER_TEXT" >"$restore_temp" \
        || ! chmod 600 "$restore_temp" \
        || ! mv -f "$restore_temp" "$MARKER" \
        || ! marker_is_exact "$MARKER"
    then
        rm -f "$restore_temp" 2>/dev/null || :
        return 1
    fi
}

INSTALL_DIR=${EML_REMOVER_HOME:-"$HOME/Library/Application Support/EML Attachment Remover"}
MARKER_NAME=.eml-attachment-remover-installation
MARKER_TEXT='EML Attachment Remover managed installation'
MARKER_BYTES=44

[ -n "$INSTALL_DIR" ] || fail 4 "Refusing an empty uninstall path."
[ ! -L "$INSTALL_DIR" ] || fail 4 "Refusing an unsafe uninstall symlink."
if [ ! -e "$INSTALL_DIR" ]; then
    printf '%s\n' "Nothing to remove."
    exit 0
fi
[ -d "$INSTALL_DIR" ] || fail 4 "Refusing a non-directory uninstall path."
canonical_directory "$INSTALL_DIR" \
    || fail 4 "Could not canonicalize the uninstall path."
CANONICAL_INSTALL_DIR=$RESOLVED_DIRECTORY
canonical_directory "$HOME" || fail 4 "Could not canonicalize HOME."
CANONICAL_HOME=$RESOLVED_DIRECTORY
case "$CANONICAL_INSTALL_DIR" in
    /|"$CANONICAL_HOME")
        fail 4 "Refusing an unsafe uninstall path."
        ;;
esac
case "$CANONICAL_HOME/" in
    "$CANONICAL_INSTALL_DIR/"*)
        fail 4 "Refusing to uninstall an ancestor of HOME."
        ;;
esac

CDPATH='' cd -P "$CANONICAL_INSTALL_DIR" \
    || fail 4 "Could not enter the verified installation directory."
canonical_directory . \
    || fail 4 "Could not verify the entered installation directory."
[ "$RESOLVED_DIRECTORY" = "$CANONICAL_INSTALL_DIR" ] \
    || fail 4 "The installation directory changed during validation."

MARKER=./$MARKER_NAME
marker_is_exact "$MARKER" \
    || fail 4 "Refusing a directory without the exact installation marker."
optional_regular_file ./remove-eml-attachments.pyz \
    || fail 4 "Refusing an unsafe installed zipapp entry."
optional_regular_file ./run-from-finder.sh \
    || fail 4 "Refusing an unsafe installed launcher entry."
! has_unknown_entry \
    || fail 4 "Refusing to remove a directory containing unknown entries."

if entry_exists ./remove-eml-attachments.pyz; then
    rm -f ./remove-eml-attachments.pyz \
        || fail 4 "Could not remove the installed zipapp; the ownership marker remains."
fi
if entry_exists ./run-from-finder.sh; then
    rm -f ./run-from-finder.sh \
        || fail 4 "Could not remove the installed launcher; the ownership marker remains."
fi

marker_is_exact "$MARKER" \
    || fail 4 "The installation marker changed during uninstall."
! has_unknown_entry \
    || fail 4 "The installation directory changed during uninstall; the ownership marker remains."
rm -f "$MARKER" \
    || fail 4 "Could not remove the installation marker."
canonical_directory . \
    || {
        restore_marker \
            || fail 4 "The installation directory moved and its marker could not be restored."
        fail 4 "The installation directory moved; its ownership marker was restored."
    }
[ "$RESOLVED_DIRECTORY" = "$CANONICAL_INSTALL_DIR" ] \
    || {
        restore_marker \
            || fail 4 "The installation directory moved and its marker could not be restored."
        fail 4 "The installation directory moved; its ownership marker was restored."
    }
canonical_directory "$CANONICAL_INSTALL_DIR" \
    || {
        restore_marker \
            || fail 4 "The installation path changed and its marker could not be restored."
        fail 4 "The installation path changed; its ownership marker was restored."
    }
[ "$RESOLVED_DIRECTORY" = "$CANONICAL_INSTALL_DIR" ] \
    || {
        restore_marker \
            || fail 4 "The installation path changed and its marker could not be restored."
        fail 4 "The installation path changed; its ownership marker was restored."
    }
if ! rmdir "$CANONICAL_INSTALL_DIR"; then
    if restore_marker; then
        fail 4 "Could not remove the installation directory; its ownership marker was restored."
    fi
    fail 4 "Could not remove the installation directory or restore its ownership marker."
fi
printf '%s\n' "Removed the managed installation."

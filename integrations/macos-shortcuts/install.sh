#!/bin/sh

set -eu

fail() {
    printf '%s\n' "$2" >&2
    exit "$1"
}

physical_directory() {
    PHYSICAL_DIRECTORY=$(
        CDPATH='' cd -P "$1" && {
            pwd -P
            printf '\037'
        }
    ) || return 1
    PHYSICAL_DIRECTORY=${PHYSICAL_DIRECTORY%?}
    PHYSICAL_DIRECTORY=${PHYSICAL_DIRECTORY%?}
}

resolve_path() {
    RESOLVED_PATH=$(
        "$PYTHON" -c \
            'from pathlib import Path; import sys; sys.stdout.write(str(Path(sys.argv[1]).expanduser().resolve(strict=False)) + "\x1f")' \
            "$1"
    ) || return 1
    RESOLVED_PATH=${RESOLVED_PATH%?}
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
    for entry in "$INSTALL_DIR"/* "$INSTALL_DIR"/.[!.]* "$INSTALL_DIR"/..?*
    do
        entry_exists "$entry" || continue
        case "$entry" in
            "$INSTALL_DIR/$MARKER_NAME"|\
            "$INSTALL_DIR/remove-eml-attachments.pyz"|\
            "$INSTALL_DIR/run-from-finder.sh"|\
            "$MARKER_TEMP"|"$ZIPAPP_TEMP"|"$RUNNER_TEMP"|\
            "$MARKER_BACKUP"|"$ZIPAPP_BACKUP"|"$RUNNER_BACKUP")
                ;;
            *)
                return 0
                ;;
        esac
    done
    return 1
}

display_path() {
    "$PYTHON" -c '
import sys, unicodedata
value = "".join(
    character
    if unicodedata.category(character) not in {"Cc", "Cf", "Cs", "Zl", "Zp"}
    else (
        f"\\x{ord(character):02x}"
        if ord(character) <= 0xff
        else f"\\u{ord(character):04x}"
        if ord(character) <= 0xffff
        else f"\\U{ord(character):08x}"
    )
    for character in sys.argv[1]
)
encoding = sys.stdout.encoding or sys.getdefaultencoding()
print(value.encode(encoding, errors="backslashreplace").decode(encoding))
' "$1"
}

shortcut_command() {
    # shellcheck disable=SC2016  # The Python source emits literal shell variables.
    "$PYTHON" -c '
import shlex, sys, unicodedata
root = sys.argv[1]
runner = root + "/run-from-finder.sh"
arguments = chr(34) + "$@" + chr(34)
unsafe_categories = {"Cc", "Cf", "Cs", "Zl", "Zp"}
if not any(unicodedata.category(character) in unsafe_categories for character in root):
    print(
        f"EML_REMOVER_HOME={shlex.quote(root)} "
        f"/bin/sh {shlex.quote(runner)} {arguments}"
    )
else:
    def escaped_bytes(value):
        return "".join(
            chr(byte) if 0x20 <= byte <= 0x7e and byte != 0x5c
            else f"\\0{byte:03o}"
            for byte in value.encode(sys.getfilesystemencoding(), "surrogateescape")
        )

    format_argument = chr(39) + "%b_" + chr(39)
    home = shlex.quote(escaped_bytes(root))
    launcher = shlex.quote(escaped_bytes(runner))
    home_variable = chr(34) + "$EML_REMOVER_SHORTCUT_HOME" + chr(34)
    runner_variable = chr(34) + "$EML_REMOVER_SHORTCUT_RUNNER" + chr(34)
    print(
        f"EML_REMOVER_SHORTCUT_HOME=$(printf {format_argument} {home}); "
        "EML_REMOVER_SHORTCUT_HOME=${EML_REMOVER_SHORTCUT_HOME%_}; "
        f"EML_REMOVER_SHORTCUT_RUNNER=$(printf {format_argument} {launcher}); "
        "EML_REMOVER_SHORTCUT_RUNNER=${EML_REMOVER_SHORTCUT_RUNNER%_}; "
        f"EML_REMOVER_HOME={home_variable} "
        f"/bin/sh {runner_variable} " + arguments
    )
' "$1"
}

physical_directory "$(dirname "$0")" \
    || fail 4 "Could not locate the integration directory."
SCRIPT_DIR=$PHYSICAL_DIRECTORY
FILESYSTEM_SUPPORT=$SCRIPT_DIR/installer-filesystem.sh
[ -f "$FILESYSTEM_SUPPORT" ] && [ ! -L "$FILESYSTEM_SUPPORT" ] \
    || fail 4 "The installer filesystem support library is not a safe regular file."
# shellcheck disable=SC1090  # The canonical source directory is computed above.
. "$FILESYSTEM_SUPPORT"
physical_directory "$SCRIPT_DIR/../.." \
    || fail 4 "Could not locate the project directory."
PROJECT_ROOT=$PHYSICAL_DIRECTORY
INSTALL_DIR=${EML_REMOVER_HOME:-"$HOME/Library/Application Support/EML Attachment Remover"}
if [ "${EML_REMOVER_ZIPAPP+x}" = x ]; then
    ZIPAPP=$EML_REMOVER_ZIPAPP
    EXPLICIT_ZIPAPP=1
else
    ZIPAPP=$PROJECT_ROOT/build/remove-eml-attachments.pyz
    EXPLICIT_ZIPAPP=0
fi
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
MARKER_NAME=.eml-attachment-remover-installation
MARKER_TEXT='EML Attachment Remover managed installation'
MARKER_BYTES=44
MARKER_TEMP=
ZIPAPP_TEMP=
RUNNER_TEMP=
MARKER_BACKUP=
ZIPAPP_BACKUP=
RUNNER_BACKUP=
INSTALL_DIRECTORY_ID=
MARKER_BACKED_UP=0
ZIPAPP_BACKED_UP=0
RUNNER_BACKED_UP=0

"$PYTHON" -c \
    'import platform, sys; raise SystemExit(platform.python_implementation() != "CPython" or sys.version_info[:2] != (3, 14))' \
    || fail 3 "CPython 3.14 was not found or was not the required implementation."
[ -n "$INSTALL_DIR" ] || fail 4 "Refusing an empty installation path."
[ ! -L "$INSTALL_DIR" ] || fail 4 "Refusing an unsafe installation symlink."
resolve_path "$INSTALL_DIR" \
    || fail 4 "Could not canonicalize the installation path."
CANONICAL_INSTALL_DIR=$RESOLVED_PATH
resolve_path "$HOME" || fail 4 "Could not canonicalize HOME."
CANONICAL_HOME=$RESOLVED_PATH
case "$CANONICAL_INSTALL_DIR" in
    /|"$CANONICAL_HOME")
        fail 4 "Refusing an unsafe installation path."
        ;;
esac
case "$CANONICAL_HOME/" in
    "$CANONICAL_INSTALL_DIR/"*)
        fail 4 "Refusing installation into an ancestor of HOME."
        ;;
esac
safe_ancestor_chain "$CANONICAL_INSTALL_DIR" \
    || fail 4 "The installation directory has an unsafe ancestor."
SHORTCUT_COMMAND=$(shortcut_command "$CANONICAL_INSTALL_DIR") \
    || fail 4 "Could not prepare the Shortcuts launcher command."

EXISTING_INSTALLATION=0
if [ -e "$CANONICAL_INSTALL_DIR" ]; then
    [ -d "$CANONICAL_INSTALL_DIR" ] && [ ! -L "$CANONICAL_INSTALL_DIR" ] \
        || fail 4 "The installation path is not a safe directory."
    INSTALL_DIRECTORY_ID=$(safe_directory_id "$CANONICAL_INSTALL_DIR") \
        || fail 4 "The installation directory is not safely owned."
    INSTALL_DIR=$CANONICAL_INSTALL_DIR
    marker_is_exact "$INSTALL_DIR/$MARKER_NAME" \
        || fail 4 "Refusing an existing directory without the exact installation marker."
    optional_regular_file "$INSTALL_DIR/remove-eml-attachments.pyz" \
        || fail 4 "Refusing an unsafe installed zipapp entry."
    optional_regular_file "$INSTALL_DIR/run-from-finder.sh" \
        || fail 4 "Refusing an unsafe installed launcher entry."
    ! has_unknown_entry \
        || fail 4 "Refusing an installation directory containing unknown entries."
    EXISTING_INSTALLATION=1
fi

if [ "$EXPLICIT_ZIPAPP" -eq 1 ]; then
    [ -f "$ZIPAPP" ] && [ ! -L "$ZIPAPP" ] \
        || fail 3 "The selected zipapp is not a safe regular file."
else
    printf '%s\n' "Building the verified zipapp."
    if ! "$PYTHON" "$PROJECT_ROOT/tools/build_zipapp.py" \
        --target "$ZIPAPP" >/dev/null
    then
        fail 3 "Could not build the zipapp with CPython 3.14."
    fi
fi
[ -f "$ZIPAPP" ] && [ ! -L "$ZIPAPP" ] \
    || fail 3 "The zipapp build did not produce a safe regular file."
[ -f "$SCRIPT_DIR/run-from-finder.sh" ] && [ ! -L "$SCRIPT_DIR/run-from-finder.sh" ] \
    || fail 4 "The launcher source is not a safe regular file."

CREATED_INSTALLATION=0
if [ "$EXISTING_INSTALLATION" -eq 0 ]; then
    "$PYTHON" -c \
        'from pathlib import Path; import os, sys; old = os.umask(0o077); Path(sys.argv[1]).mkdir(mode=0o700, parents=True, exist_ok=False); os.umask(old)' \
        "$CANONICAL_INSTALL_DIR" \
        || fail 4 "Could not create the installation directory."
    CREATED_INSTALLATION=1
fi
resolve_path "$CANONICAL_INSTALL_DIR" \
    || fail 4 "Could not verify the installation path."
INSTALL_DIR=$RESOLVED_PATH
if [ "$INSTALL_DIR" != "$CANONICAL_INSTALL_DIR" ] \
    || [ -L "$INSTALL_DIR" ] \
    || [ ! -d "$INSTALL_DIR" ]; then
    fail 4 "The installation path changed during validation."
fi
safe_ancestor_chain "$INSTALL_DIR" \
    || fail 4 "The installation directory has an unsafe ancestor."
VERIFIED_DIRECTORY_ID=$(safe_directory_id "$INSTALL_DIR") \
    || fail 4 "The installation directory is not safely owned."
if [ -n "$INSTALL_DIRECTORY_ID" ] \
    && [ "$VERIFIED_DIRECTORY_ID" != "$INSTALL_DIRECTORY_ID" ]; then
    fail 4 "The installation directory identity changed during validation."
fi
INSTALL_DIRECTORY_ID=$VERIFIED_DIRECTORY_ID
set_directory_mode "$INSTALL_DIR" \
    || fail 4 "Could not secure the installation directory."

ROLLBACK_ACTIVE=0
cleanup() {
    cleanup_status=$?
    trap - EXIT HUP INT TERM
    [ -z "$ZIPAPP_TEMP" ] \
        || remove_regular_file "$ZIPAPP_TEMP" 2>/dev/null \
        || :
    [ -z "$RUNNER_TEMP" ] \
        || remove_regular_file "$RUNNER_TEMP" 2>/dev/null \
        || :
    [ -z "$MARKER_TEMP" ] \
        || remove_regular_file "$MARKER_TEMP" 2>/dev/null \
        || :
    if [ "$ROLLBACK_ACTIVE" -eq 1 ]; then
        ROLLBACK_OK=1
        if [ "$ZIPAPP_BACKED_UP" -eq 1 ]; then
            optional_regular_file "$INSTALL_DIR/remove-eml-attachments.pyz" \
                && remove_regular_file \
                    "$INSTALL_DIR/remove-eml-attachments.pyz" 2>/dev/null \
                || ROLLBACK_OK=0
            if [ "$ROLLBACK_OK" -eq 1 ]; then
                replace_file \
                    "$ZIPAPP_BACKUP" \
                    "$INSTALL_DIR/remove-eml-attachments.pyz" 2>/dev/null \
                    || ROLLBACK_OK=0
            fi
        fi
        if [ "$RUNNER_BACKED_UP" -eq 1 ]; then
            optional_regular_file "$INSTALL_DIR/run-from-finder.sh" \
                && remove_regular_file \
                    "$INSTALL_DIR/run-from-finder.sh" 2>/dev/null \
                || ROLLBACK_OK=0
            if [ "$ROLLBACK_OK" -eq 1 ]; then
                replace_file \
                    "$RUNNER_BACKUP" \
                    "$INSTALL_DIR/run-from-finder.sh" 2>/dev/null \
                    || ROLLBACK_OK=0
            fi
        fi
        if [ "$MARKER_BACKED_UP" -eq 1 ] && [ "$ROLLBACK_OK" -eq 1 ]; then
            if entry_exists "$INSTALL_DIR/$MARKER_NAME"; then
                remove_exact_marker "$INSTALL_DIR/$MARKER_NAME" 2>/dev/null \
                    || ROLLBACK_OK=0
            fi
        fi
        if [ "$MARKER_BACKED_UP" -eq 1 ] && [ "$ROLLBACK_OK" -eq 1 ]; then
            replace_file \
                "$MARKER_BACKUP" \
                "$INSTALL_DIR/$MARKER_NAME" 2>/dev/null \
                || ROLLBACK_OK=0
        fi
        if [ "$ROLLBACK_OK" -eq 1 ]; then
            MARKER_BACKUP=
            ZIPAPP_BACKUP=
            RUNNER_BACKUP=
            MARKER_BACKED_UP=0
            ZIPAPP_BACKED_UP=0
            RUNNER_BACKED_UP=0
            ROLLBACK_ACTIVE=0
        else
            if marker_is_exact "$INSTALL_DIR/$MARKER_NAME"; then
                remove_exact_marker "$INSTALL_DIR/$MARKER_NAME" 2>/dev/null || :
            fi
            printf '%s\n' \
                "Warning: the prior installation could not be rolled back exactly; recovery files remain for manual inspection." \
                >&2
        fi
    fi
    if [ "$CREATED_INSTALLATION" -eq 1 ] \
        && ! entry_exists "$INSTALL_DIR/$MARKER_NAME" \
        && ! has_unknown_entry
    then
        optional_regular_file "$INSTALL_DIR/remove-eml-attachments.pyz" \
            && remove_regular_file \
                "$INSTALL_DIR/remove-eml-attachments.pyz" 2>/dev/null \
            || :
        optional_regular_file "$INSTALL_DIR/run-from-finder.sh" \
            && remove_regular_file \
                "$INSTALL_DIR/run-from-finder.sh" 2>/dev/null \
            || :
        CURRENT_DIRECTORY_ID=$(safe_directory_id "$INSTALL_DIR" 2>/dev/null) \
            || CURRENT_DIRECTORY_ID=
        if [ "$CURRENT_DIRECTORY_ID" = "$INSTALL_DIRECTORY_ID" ]; then
            rmdir "$INSTALL_DIR" 2>/dev/null || :
        fi
    fi
    if [ "$ROLLBACK_ACTIVE" -eq 0 ]; then
        [ -z "$MARKER_BACKUP" ] \
            || remove_regular_file "$MARKER_BACKUP" 2>/dev/null \
            || :
        [ -z "$ZIPAPP_BACKUP" ] \
            || remove_regular_file "$ZIPAPP_BACKUP" 2>/dev/null \
            || :
        [ -z "$RUNNER_BACKUP" ] \
            || remove_regular_file "$RUNNER_BACKUP" 2>/dev/null \
            || :
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

MARKER_TEMP=$(mktemp "$INSTALL_DIR/.marker.XXXXXX" 2>/dev/null) \
    || fail 7 "Could not stage the installation marker."
ZIPAPP_TEMP=$(mktemp "$INSTALL_DIR/.zipapp.XXXXXX" 2>/dev/null) \
    || fail 7 "Could not stage the zipapp."
RUNNER_TEMP=$(mktemp "$INSTALL_DIR/.runner.XXXXXX" 2>/dev/null) \
    || fail 7 "Could not stage the Finder launcher."
if [ "$EXISTING_INSTALLATION" -eq 1 ]; then
    MARKER_BACKUP=$(mktemp "$INSTALL_DIR/.marker-backup.XXXXXX" 2>/dev/null) \
        || fail 7 "Could not reserve the marker rollback path."
    remove_regular_file "$MARKER_BACKUP"
    if entry_exists "$INSTALL_DIR/remove-eml-attachments.pyz"; then
        ZIPAPP_BACKUP=$(mktemp "$INSTALL_DIR/.zipapp-backup.XXXXXX" 2>/dev/null) \
            || fail 7 "Could not reserve the zipapp rollback path."
        remove_regular_file "$ZIPAPP_BACKUP"
    fi
    if entry_exists "$INSTALL_DIR/run-from-finder.sh"; then
        RUNNER_BACKUP=$(mktemp "$INSTALL_DIR/.runner-backup.XXXXXX" 2>/dev/null) \
            || fail 7 "Could not reserve the launcher rollback path."
        remove_regular_file "$RUNNER_BACKUP"
    fi
fi
printf '%s\n' "$MARKER_TEXT" >"$MARKER_TEMP"
copy_regular_file "$ZIPAPP" "$ZIPAPP_TEMP"
copy_regular_file "$SCRIPT_DIR/run-from-finder.sh" "$RUNNER_TEMP"
set_regular_mode "$MARKER_TEMP" 600
set_regular_mode "$ZIPAPP_TEMP" 644
set_regular_mode "$RUNNER_TEMP" 755

if [ "$EXISTING_INSTALLATION" -eq 1 ]; then
    marker_is_exact "$INSTALL_DIR/$MARKER_NAME" \
        || fail 4 "The installation marker changed during staging."
fi
optional_regular_file "$INSTALL_DIR/remove-eml-attachments.pyz" \
    || fail 4 "The installed zipapp entry changed during staging."
optional_regular_file "$INSTALL_DIR/run-from-finder.sh" \
    || fail 4 "The installed launcher entry changed during staging."
! has_unknown_entry \
    || fail 4 "The installation directory changed during staging."

if [ "$EXISTING_INSTALLATION" -eq 1 ]; then
    ROLLBACK_ACTIVE=1
    if [ -n "$ZIPAPP_BACKUP" ]; then
        replace_file \
            "$INSTALL_DIR/remove-eml-attachments.pyz" \
            "$ZIPAPP_BACKUP"
        ZIPAPP_BACKED_UP=1
    fi
    if [ -n "$RUNNER_BACKUP" ]; then
        replace_file "$INSTALL_DIR/run-from-finder.sh" "$RUNNER_BACKUP"
        RUNNER_BACKED_UP=1
    fi
    replace_file "$INSTALL_DIR/$MARKER_NAME" "$MARKER_BACKUP"
    MARKER_BACKED_UP=1
fi
replace_file "$ZIPAPP_TEMP" "$INSTALL_DIR/remove-eml-attachments.pyz"
ZIPAPP_TEMP=
replace_file "$RUNNER_TEMP" "$INSTALL_DIR/run-from-finder.sh"
RUNNER_TEMP=
replace_file "$MARKER_TEMP" "$INSTALL_DIR/$MARKER_NAME"
MARKER_TEMP=
ROLLBACK_ACTIVE=0
[ -z "$MARKER_BACKUP" ] || remove_regular_file "$MARKER_BACKUP"
[ -z "$ZIPAPP_BACKUP" ] || remove_regular_file "$ZIPAPP_BACKUP"
[ -z "$RUNNER_BACKUP" ] || remove_regular_file "$RUNNER_BACKUP"
MARKER_BACKUP=
ZIPAPP_BACKUP=
RUNNER_BACKUP=
trap - EXIT HUP INT TERM

printf '%s' "Installed to: "
display_path "$INSTALL_DIR"
printf '%s\n' "Name the shortcut: Create Text-Only EML Copy"
printf '%s\n' "Paste this into a Shortcuts 'Run Shell Script' action:"
printf '%s\n' "$SHORTCUT_COMMAND"
printf '%s\n' "Set 'Input' to 'Shortcut Input' and 'Pass Input' to 'as arguments'."

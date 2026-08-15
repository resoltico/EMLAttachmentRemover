#!/bin/sh
# Safe filesystem primitives sourced by install.sh.

# shellcheck disable=SC2154  # Values are initialized by install.sh before use.
replace_file() {
    "$PYTHON" -c '
import os, stat, sys
from pathlib import Path
source = Path(sys.argv[1])
destination = Path(sys.argv[2])
if source.parent != destination.parent:
    raise OSError("replacement paths have different parents")
expected = tuple(int(value) for value in sys.argv[3].split(":"))
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
directory_fd = os.open(destination.parent, flags)
try:
    directory_stat = os.fstat(directory_fd)
    if (directory_stat.st_dev, directory_stat.st_ino) != expected:
        raise OSError("installation directory identity changed")
    source_stat = os.stat(source.name, dir_fd=directory_fd, follow_symlinks=False)
    if not stat.S_ISREG(source_stat.st_mode):
        raise OSError("replacement source is not a regular file")
    try:
        destination_stat = os.stat(
            destination.name,
            dir_fd=directory_fd,
            follow_symlinks=False,
        )
    except FileNotFoundError:
        pass
    else:
        if not stat.S_ISREG(destination_stat.st_mode):
            raise OSError("replacement destination is not a regular file")
    os.replace(
        source.name,
        destination.name,
        src_dir_fd=directory_fd,
        dst_dir_fd=directory_fd,
    )
finally:
    os.close(directory_fd)
' "$1" "$2" "$INSTALL_DIRECTORY_ID"
}

copy_regular_file() {
    "$PYTHON" -c '
import os, shutil, stat, sys
from pathlib import Path
no_follow = getattr(os, "O_NOFOLLOW", 0)
source_fd = os.open(sys.argv[1], os.O_RDONLY | no_follow)
try:
    if not stat.S_ISREG(os.fstat(source_fd).st_mode):
        raise OSError("source is not a regular file")
    destination = Path(sys.argv[2])
    expected = tuple(int(value) for value in sys.argv[3].split(":"))
    directory_flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | no_follow
    directory_fd = os.open(destination.parent, directory_flags)
    try:
        directory_stat = os.fstat(directory_fd)
        if (directory_stat.st_dev, directory_stat.st_ino) != expected:
            raise OSError("installation directory identity changed")
        destination_fd = os.open(
            destination.name,
            os.O_WRONLY | os.O_TRUNC | no_follow,
            dir_fd=directory_fd,
        )
        try:
            if not stat.S_ISREG(os.fstat(destination_fd).st_mode):
                raise OSError("destination is not a regular file")
            with os.fdopen(source_fd, "rb", closefd=False) as source:
                with os.fdopen(destination_fd, "wb", closefd=False) as output:
                    shutil.copyfileobj(source, output)
                    output.flush()
                    os.fsync(destination_fd)
        finally:
            os.close(destination_fd)
    finally:
        os.close(directory_fd)
finally:
    os.close(source_fd)
' "$1" "$2" "$INSTALL_DIRECTORY_ID"
}

safe_directory_id() {
    "$PYTHON" -c '
import os, stat, sys
value = os.lstat(sys.argv[1])
if not stat.S_ISDIR(value.st_mode) or value.st_uid != os.geteuid():
    raise OSError("installation directory is not owned by this user")
if stat.S_IMODE(value.st_mode) & 0o022:
    raise OSError("installation directory is writable by another user")
print(f"{value.st_dev}:{value.st_ino}")
' "$1"
}

safe_ancestor_chain() {
    "$PYTHON" -c '
import os, stat, sys
from pathlib import Path
path = Path(sys.argv[1]).parent
while True:
    try:
        value = os.lstat(path)
    except FileNotFoundError:
        path = path.parent
        continue
    if not stat.S_ISDIR(value.st_mode):
        raise OSError("installation ancestor is not a directory")
    mode = stat.S_IMODE(value.st_mode)
    if mode & 0o022 and not mode & stat.S_ISVTX:
        raise OSError("installation ancestor is writable by another user")
    if path.parent == path:
        break
    path = path.parent
' "$1"
}

set_directory_mode() {
    "$PYTHON" -c '
import os, sys
expected = tuple(int(value) for value in sys.argv[2].split(":"))
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
directory_fd = os.open(sys.argv[1], flags)
try:
    value = os.fstat(directory_fd)
    if (value.st_dev, value.st_ino) != expected:
        raise OSError("installation directory identity changed")
    os.fchmod(directory_fd, 0o700)
finally:
    os.close(directory_fd)
' "$1" "$INSTALL_DIRECTORY_ID"
}
set_regular_mode() {
    "$PYTHON" -c '
import os, stat, sys
from pathlib import Path
path = Path(sys.argv[1])
expected = tuple(int(value) for value in sys.argv[3].split(":"))
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
directory_fd = os.open(path.parent, flags)
try:
    directory_stat = os.fstat(directory_fd)
    if (directory_stat.st_dev, directory_stat.st_ino) != expected:
        raise OSError("installation directory identity changed")
    file_fd = os.open(
        path.name,
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
        dir_fd=directory_fd,
    )
    try:
        if not stat.S_ISREG(os.fstat(file_fd).st_mode):
            raise OSError("mode target is not a regular file")
        os.fchmod(file_fd, int(sys.argv[2], 8))
    finally:
        os.close(file_fd)
finally:
    os.close(directory_fd)
' "$1" "$2" "$INSTALL_DIRECTORY_ID"
}

remove_regular_file() {
    "$PYTHON" -c '
import os, stat, sys
from pathlib import Path
path = Path(sys.argv[1])
expected = tuple(int(value) for value in sys.argv[2].split(":"))
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
directory_fd = os.open(path.parent, flags)
try:
    directory_stat = os.fstat(directory_fd)
    if (directory_stat.st_dev, directory_stat.st_ino) != expected:
        raise OSError("installation directory identity changed")
    try:
        value = os.stat(path.name, dir_fd=directory_fd, follow_symlinks=False)
    except FileNotFoundError:
        pass
    else:
        if not stat.S_ISREG(value.st_mode):
            raise OSError("cleanup entry is not a regular file")
        os.unlink(path.name, dir_fd=directory_fd)
finally:
    os.close(directory_fd)
' "$1" "$INSTALL_DIRECTORY_ID"
}

remove_exact_marker() {
    "$PYTHON" -c '
import os, stat, sys
from pathlib import Path
path = Path(sys.argv[1])
expected = tuple(int(value) for value in sys.argv[2].split(":"))
expected_bytes = b"EML Attachment Remover managed installation\n"
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
directory_fd = os.open(path.parent, flags)
try:
    directory_stat = os.fstat(directory_fd)
    if (directory_stat.st_dev, directory_stat.st_ino) != expected:
        raise OSError("installation directory identity changed")
    marker_fd = os.open(
        path.name,
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
        dir_fd=directory_fd,
    )
    try:
        if not stat.S_ISREG(os.fstat(marker_fd).st_mode):
            raise OSError("marker is not a regular file")
        with os.fdopen(marker_fd, "rb", closefd=False) as marker:
            if marker.read() != expected_bytes:
                raise OSError("marker contents changed")
    finally:
        os.close(marker_fd)
    os.unlink(path.name, dir_fd=directory_fd)
finally:
    os.close(directory_fd)
' "$1" "$INSTALL_DIRECTORY_ID"
}

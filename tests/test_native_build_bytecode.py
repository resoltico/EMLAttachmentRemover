"""Native metadata builds cannot create cache files in their source checkout."""

from __future__ import annotations

import os
import plistlib
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_native_metadata_step_keeps_the_checkout_free_of_bytecode(
    tmp_path: Path,
) -> None:
    """Run the actual build-script metadata program without a protective parent env."""
    script = (ROOT / "integrations/macos-ui/build.sh").read_text()
    invocation = next(
        line for line in script.splitlines() if '"$PROJECT_ROOT/pyproject.toml"' in line
    )
    arguments = shlex.split(invocation)
    flags = arguments[1 : arguments.index("$PROJECT_ROOT/pyproject.toml")]
    program = script.split("<<'PY'\n", 1)[1].split("\nPY\n", 1)[0]
    source = tmp_path / "source"
    (source / "tools").mkdir(parents=True)
    for name in ["pyproject.toml", "LICENSE", "tools/macos_metadata.py"]:
        shutil.copy2(ROOT / name, source / name)
    icon = tmp_path / "icon.plist"
    icon.write_bytes(plistlib.dumps({}))
    info = tmp_path / "Info.plist"
    environment = os.environ.copy()
    for name in ["PYTHONDONTWRITEBYTECODE", "PYTHONPATH"]:
        environment.pop(name, None)
    subprocess.run(
        [
            sys.executable,
            *flags,
            str(source / "pyproject.toml"),
            str(info),
            str(icon),
            "external",
            "arm64",
        ],
        input=program,
        text=True,
        check=True,
        capture_output=True,
        cwd=source,
        env=environment,
        timeout=30,
    )
    assert plistlib.loads(info.read_bytes())["CFBundleIdentifier"] == (
        "io.github.resoltico.emlattachmentremover"
    )
    assert not list(source.rglob("__pycache__"))

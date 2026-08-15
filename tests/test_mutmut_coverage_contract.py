"""Prove Mutmut can recover lines measured under its copied source tree."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
MUTATION_COVERAGE_CONFIG = PROJECT_ROOT / "tools" / "mutmut.coveragerc"
MEASUREMENT_PROGRAM = """
import json
import runpy
import sys
from contextlib import chdir
from pathlib import Path

from coverage import Coverage
from mutmut.code_coverage import get_covered_lines_for_file

root = Path(sys.argv[1])
mutant_root = root / "mutants"
source = mutant_root / "tools" / "public_module.py"
with chdir(mutant_root):
    reporter = Coverage(data_file=None)
    with reporter.collect():
        namespace = runpy.run_path(
            str(source),
            run_name="public_mutation_coverage_fixture",
        )
data = reporter.get_data()
covered = {
    filename: set(data.lines(filename) or ())
    for filename in data.measured_files()
}
with chdir(root):
    mutation_lines = get_covered_lines_for_file(
        "tools/public_module.py",
        covered,
    )
print(
    json.dumps({
        "result": namespace["PUBLIC_RESULT"],
        "lines": sorted(mutation_lines or ()),
    })
)
"""


class MutmutCoverageContractTests(unittest.TestCase):
    """Verify the absolute filename contract between Coverage and Mutmut 3.7."""

    def test_mutmut_finds_lines_measured_from_its_working_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            mutant_root = root / "mutants"
            source = mutant_root / "tools" / "public_module.py"
            source.parent.mkdir(parents=True)
            source.write_text(
                "PUBLIC_VALUE = 40\nPUBLIC_RESULT = PUBLIC_VALUE + 2\n",
                encoding="utf-8",
            )
            environment = {
                name: value
                for name, value in os.environ.items()
                if not name.startswith("COVERAGE_")
            }
            environment["COVERAGE_RCFILE"] = str(MUTATION_COVERAGE_CONFIG)
            completed = subprocess.run(
                (sys.executable, "-c", MEASUREMENT_PROGRAM, str(root)),
                check=True,
                cwd=PROJECT_ROOT,
                env=environment,
                capture_output=True,
                text=True,
                timeout=30,
            )
        result = json.loads(completed.stdout)
        self.assertEqual(result, {"result": 42, "lines": [1, 2]})


if __name__ == "__main__":
    unittest.main(verbosity=2)

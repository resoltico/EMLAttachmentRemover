"""Delivery jobs share the declared hosted image and exact Xcode installation."""

from __future__ import annotations

import importlib
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_producer_and_publisher_use_declared_image_and_verified_sdk() -> None:
    pin = tomllib.loads((ROOT / "integrations/macos-ui/toolchain.toml").read_text())[
        "delivery"
    ]
    targets = [
        ("release.yml", "build"),
        ("release.yml", "publish"),
        ("macos-compatibility.yml", "producer"),
    ]
    for workflow, job in targets:
        config = importlib.import_module("yaml").safe_load(
            (ROOT / ".github/workflows" / workflow).read_text()
        )
        definition = config["jobs"][job]
        assert definition["runs-on"] == pin["runner"]
        steps = definition["steps"]
        selection = next(
            n for n, step in enumerate(steps) if "ci-sdk.sh" in step.get("run", "")
        )
        use = (
            next(
                n
                for n, step in enumerate(steps)
                if any(
                    name in step.get("run", "")
                    for name in [
                        "release_delivery",
                        "download-artifact",
                        "swift-toolchain",
                    ]
                )
            )
            if job != "publish"
            else next(
                n
                for n, step in enumerate(steps)
                if "release_delivery" in step.get("run", "")
            )
        )
        assert selection < use
        assert "$GITHUB_ENV" in steps[selection]["run"]
    assert pin["xcode-version"] == "26.6"
    assert pin["xcode-build"] == "17F113"

"""Only run measuring-kit integration tests with this checkout's binaries."""
import os
import re
import subprocess
import unittest
from pathlib import Path


def current_bins(root: Path) -> Path:
    """Check both build stamps and require a clean source tree; never start an old daemon."""
    try:
        head = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, stderr=subprocess.DEVNULL, text=True
        ).strip()
        dirty = subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=normal"], cwd=root, text=True
        ).strip()
        if dirty:
            raise unittest.SkipTest("cannot verify binaries against an edited tree; commit and rebuild theseus and theseusd")
        debug = root / "target" / "debug"
        daemon = debug / "theseusd"
        if not daemon.is_file() or not (debug / "theseus").is_file():
            raise unittest.SkipTest("cannot verify current binaries: target/debug lacks theseus or theseusd; build this tree first")
        stamps = re.findall(rb"theseus-build-stamp:([0-9a-f]{40}):", daemon.read_bytes())
        if stamps != [head.encode()]:
            raise unittest.SkipTest("cannot verify current daemon: target/debug/theseusd was not stamped with this HEAD; rebuild this tree")
        cli = subprocess.check_output(
            [str(debug / "theseus")], env={**os.environ, "THESEUS_CLI_STAMP": "1"},
            stderr=subprocess.DEVNULL, text=True, timeout=5,
        ).strip()
        if cli != head:
            raise unittest.SkipTest("cannot verify current CLI: target/debug/theseus was not stamped with this HEAD; rebuild this tree")
        return debug
    except (OSError, subprocess.SubprocessError) as error:
        raise unittest.SkipTest("cannot verify this tree's binary stamps; build theseus and theseusd in a clean git checkout") from error

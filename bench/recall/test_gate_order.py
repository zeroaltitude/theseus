"""The gate runs the recall tests on freshly built binaries (theseus-a3woz).

test_drive.py drives `target/debug`'s theseus, theseusd, theseus-index and
theseus-sim, and skips itself when one of them is missing. From ahead of the
`test build` phase that builds them, a warm lane therefore tested the previous
head's binaries and said nothing: on 2026-11-03 a renamed config key left the
stale daemon failing its TOML parse and the gate stayed green. So the `recall
tests` phase must come after `test build`, and this holds it there.
"""

import re
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = REPO / "scripts" / "gate.sh"


def phase_order() -> list[str]:
    """Each `phase <name> …` of gate.sh's own flow, in the order it runs. Only
    the unindented calls: an indented one is inside a function (`locked_checks`,
    `machine_checks`), whose place in the flow its own line decides."""
    names = []
    for line in GATE.read_text().splitlines():
        m = re.match(r'phase\s+(?:"([^"]+)"|(\S+))', line)
        if m:
            names.append(m.group(1) or m.group(2))
    return names


class GateOrder(unittest.TestCase):
    def test_the_recall_tests_run_after_the_test_build(self):
        names = phase_order()
        self.assertIn("recall tests", names, f"no `recall tests` phase in {GATE}'s flow")
        self.assertIn("test build", names, f"no `test build` phase in {GATE}'s flow")
        self.assertGreater(
            names.index("recall tests"), names.index("test build"),
            "gate.sh runs `recall tests` before `test build`, so bench/recall's driver tests either skip "
            f"(no binaries) or drive the previous head's: {names}")

"""The gate runs the recall tests on freshly built binaries (theseus-a3woz).

test_drive.py drives `target/debug`'s theseus, theseusd, theseus-index and
theseus-sim, and skips itself when one of them is missing. From ahead of the
`test build` phase that builds them, a warm lane therefore tested the previous
head's binaries and said nothing: on 2026-11-03 a renamed config key left the
stale daemon failing its TOML parse and the gate stayed green. So the `recall
tests` phase must come after `test build`, and this holds it there.
"""

import re
import subprocess
import tempfile
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

    def test_real_daemon_tests_run_after_build_and_recall(self):
        names = phase_order()
        self.assertIn("real daemon tests", names)
        self.assertIn('phase "real daemon tests" real_daemon_tests', GATE.read_text())
        self.assertGreater(names.index("real daemon tests"), names.index("test build"))
        self.assertEqual(names.index("real daemon tests"), names.index("recall tests") + 1)

    def test_real_daemon_phase_keeps_skip_and_warning_in_its_callers_shell(self):
        body = re.search(r"^real_daemon_tests\(\) \{.*?^\}", GATE.read_text(), re.M | re.S).group()
        for case in ("pass", "skip", "skip_quoted", "fail"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as tmp:
                script = """
set -eu
resolve_target_dir() { target_dir=/tmp/test-target; }
phase_label=""; skipped=""; warned=""
skip() { skipped=yes; }
warn() { warned=yes; }
python3() {
  test "$CARGO_TARGET_DIR" = /tmp/test-target
  test "$PYTHONDONTWRITEBYTECODE" = 1
  case "$1" in -m) ;; *) return 9 ;; esac
  case "$mode" in
    pass) echo 'Ran 2 tests'; echo OK ;;
    skip) echo "test_example ... skipped 'missing current binaries'" ;;
    skip_quoted) echo 'test_example ... skipped "cannot verify binaries"' ;;
    fail) echo FAILED; return 1 ;;
  esac
}
""" + body + """
real_daemon_tests
printf 'RESULT label=%s skip=%s warn=%s\n' "$phase_label" "$skipped" "$warned"
"""
                result = subprocess.run(["bash", "-c", 'gate_tmp="$1"; mode="$2"; ' + script, "phase", tmp, case], text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                expected = {
                    "pass": "RESULT label= skip= warn=",
                    "skip": "RESULT label=real daemon tests skipped skip=yes warn=",
                    "skip_quoted": "RESULT label=real daemon tests skipped skip=yes warn=",
                    "fail": "RESULT label=real daemon tests warned skip= warn=yes",
                }[case]
                self.assertIn(expected, result.stdout)

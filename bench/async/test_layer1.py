"""The async bench's Layer 1 drivers (theseus-2wxa), offline.

    python3 -m unittest discover -s bench/async

The stand-in's rules, each harness's command, and the judgment of a count's
runs; and, with `ASYNC_E2E_BIN=<dir>` naming a build's binaries (theseus,
theseusd, theseus-sim), a real run of Theseus's driver on the stand-in.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import layer1  # noqa: E402


class Rules(unittest.TestCase):
    def test_each_harness_and_count_has_one_rule_and_no_mark_holds_another(self):
        rules = layer1.rules()
        self.assertEqual(len(rules), len(layer1.HARNESSES) * len(layer1.COUNTS))
        for r in rules:
            self.assertEqual(set(r), {"when", "calls"})
            # The stand-in takes the first rule the text holds: only its own.
            holders = [o["when"] for o in rules if o["when"] in r["when"]]
            self.assertEqual(holders, [r["when"]])
        by = {r["when"]: r for r in rules}
        r = by[layer1.mark("pi", 16)]
        self.assertEqual(len(r["calls"]), 16)
        self.assertEqual(r["calls"][0], {"name": "bash", "input": {"command": "sleep 2"}})
        self.assertEqual(by[layer1.mark("claude-code", 4)]["calls"][0]["name"], "Bash")
        self.assertEqual(by[layer1.mark("theseus", 1)]["calls"][0],
                         {"name": "proc_run", "input": {"argv": ["sleep", "2"]}})
        for h in layer1.HARNESSES:
            self.assertIn(layer1.mark(h, 8), layer1.prompt(h, 8))


class Commands(unittest.TestCase):
    def spec(self, harness: str) -> dict:
        return layer1.command(harness, 4, "http://127.0.0.1:9448", Path("/tmp/l1-x"), "/opt/b")

    def test_each_harness_points_at_the_stand_in_by_its_own_setting(self):
        cc = self.spec("claude-code")
        self.assertEqual(cc["argv"][0], "claude")
        self.assertEqual(cc["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:9448")
        self.assertEqual(cc["env"]["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1")
        self.assertIn(layer1.prompt("claude-code", 4), cc["argv"])
        pi = self.spec("pi")
        models = json.loads(pi["files"]["/tmp/l1-x/pi-agent/models.json"])
        self.assertEqual(models["providers"]["layer1"]["baseUrl"], "http://127.0.0.1:9448")
        self.assertEqual(pi["env"]["PI_CODING_AGENT_DIR"], "/tmp/l1-x/pi-agent")
        oc = self.spec("opencode")
        cfg = json.loads(oc["files"]["/tmp/l1-x/opencode.json"])
        self.assertEqual(cfg["provider"]["anthropic"]["options"]["baseURL"], "http://127.0.0.1:9448")
        self.assertEqual(oc["env"]["OPENCODE_CONFIG"], "/tmp/l1-x/opencode.json")
        self.assertEqual(oc["env"]["OPENCODE_DISABLE_MODELS_FETCH"], "1")
        th = self.spec("theseus")
        self.assertEqual(th["argv"][:3], ["/opt/b/theseus", "--spawn", "/opt/b/theseusd"])
        self.assertEqual(th["stdin"], layer1.prompt("theseus", 4))
        toml = th["files"]["/tmp/l1-x/theseus.toml"]
        self.assertIn('api_base = "http://127.0.0.1:9448"', toml)
        self.assertIn("proc_sync_secs = 60", toml)


class Judge(unittest.TestCase):
    def test_a_count_is_a_number_only_when_every_run_ran_its_calls(self):
        ok = layer1.judge(4, [{"wall_s": 2.4, "exit": 0}, {"wall_s": 2.2, "exit": 0}, {"wall_s": 2.9, "exit": 0}])
        self.assertEqual((ok["ok"], ok["wall_s"], ok["serial_s"], ok["ideal_s"]), (True, 2.4, 8, 2))
        failed = layer1.judge(4, [{"wall_s": 2.4, "exit": 0}, {"wall_s": 0.4, "exit": 1, "why": "401"}])
        self.assertEqual((failed["ok"], failed["wall_s"], failed["why"]), (False, None, "401"))
        # Faster than one call's sleep: the calls never ran.
        fast = layer1.judge(2, [{"wall_s": 0.6, "exit": 0}])
        self.assertFalse(fast["ok"])
        self.assertIn("never ran", fast["why"])

    def test_a_harness_not_installed_is_not_measurable_and_says_so(self):
        with tempfile.TemporaryDirectory() as d:
            spec = layer1.command("pi", 1, "http://127.0.0.1:1", Path(d), None)
            spec["argv"][0] = "/nonexistent/pi"
            r = layer1.run_once(spec, 5)
            self.assertEqual(r["wall_s"], None)
            self.assertIn("not installed", r["why"])


@unittest.skipIf(not os.environ.get("ASYNC_E2E_BIN"), "set ASYNC_E2E_BIN to a build's binaries")
class EndToEnd(unittest.TestCase):
    def test_theseus_runs_each_counts_calls_on_the_stand_in(self):
        b = Path(os.environ["ASYNC_E2E_BIN"]).resolve()
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "theseus.json"
            code = layer1.main(["run", "--harness", "theseus", "--bin", str(b), "--sim", str(b / "theseus-sim"),
                                "--base", "http://127.0.0.1:9463", "--runs", "1", "--counts", "1,2",
                                "--out", str(out)])
            got = json.loads(out.read_text())
            self.assertEqual(code, 0, got)
            walls = {c["n"]: c["wall_s"] for c in got["counts"]}
            self.assertGreaterEqual(walls[1], 2.0)
            self.assertGreaterEqual(walls[2], 2.0)


if __name__ == "__main__":
    unittest.main()

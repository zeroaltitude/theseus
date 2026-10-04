"""Tests for the Harbor adapter's parts (theseus-n88g.3).

    python3 -m unittest discover -s bench/harbor

The standard library runs every test but the ones that need Harbor, which
skip; with Harbor installed (its venv's python), those check the trajectory
against Harbor's own ATIF model, and that the adapter loads.
"""

from __future__ import annotations

import json
import os
import re
import stat
import subprocess
import tempfile
import time
import tomllib
import unittest
from pathlib import Path

import theseus_atif as atif
import theseus_bench as tb

REPO = Path(__file__).resolve().parents[2]

try:
    from harbor.models.trajectories import Trajectory
    from harbor.utils.trajectory_utils import compute_model_usage
except ImportError:  # Harbor is not installed: those tests skip.
    Trajectory = None


class Profile(unittest.TestCase):
    def test_a_trial_sets_its_lines_and_the_rest_stays(self):
        system = 'Work on your own.\nUse ["bash", "-lc", "..."] when a shell helps; say "done".'
        values = tb.settings(
            "claude-sonnet-5-5",
            "/app/a task",
            max_loops=50,
            spend_limit_usd=0.4,
            proc_sync_secs=600,
            system=system,
            api_base="http://127.0.0.1:9/invented",
        )
        text = tb.profile(tb.PROFILE.read_text(), values)
        cfg = tomllib.loads(text)
        self.assertEqual(cfg["model"]["model"], "claude-sonnet-5-5")
        self.assertEqual(cfg["profiles"]["bench"]["max_loops"], 50)
        self.assertEqual(cfg["profiles"]["bench"]["system"], system)
        self.assertEqual(cfg["kernel"]["spend_limit_usd"], 0.4)
        self.assertEqual(cfg["tools"]["projects_dir"], "/app/a task")
        self.assertEqual(cfg["tools"]["proc_sync_secs"], 600)
        self.assertEqual(cfg["model"]["api_base"], "http://127.0.0.1:9/invented")
        # What the trial does not set is the profile's.
        self.assertEqual(cfg["secrets"], {"anthropic_api_key": "env:ANTHROPIC_API_KEY"})
        self.assertEqual(cfg["policy"]["enforcement"], "open")
        self.assertEqual(cfg["tools"]["roots"], ["/"])
        self.assertFalse(cfg["web"]["enabled"] or cfg["discord"]["enabled"])
        self.assertNotIn("op://", text)

    def test_a_missing_key_goes_into_its_table_and_a_missing_table_is_added(self):
        text = tb.profile(
            "[model]\nprovider = \"anthropic\"\n\n[tools]\nroots = [\"/\"]\n",
            {("tools", "projects_dir"): "/app", ("kernel", "spend_limit_usd"): 1.0},
        )
        cfg = tomllib.loads(text)
        self.assertEqual(cfg["tools"], {"projects_dir": "/app", "roots": ["/"]})
        self.assertEqual(cfg["kernel"], {"spend_limit_usd": 1.0})


class ExitCodes(unittest.TestCase):
    def test_the_table_is_the_clis(self):
        """ENDS says what `theseus ask` exits with: read from outcome.rs, so
        the two cannot drift."""
        src = (REPO / "crates/theseus/src/outcome.rs").read_text()
        body = src[src.index("pub const fn code(self)"):]
        body = body[: body.index("}\n    }")]
        arms = dict(re.findall(r"Self::(\w+) => (\d+)", body))
        names = {"Done": "done", "Failed": "failed", "SpendLimit": "spend_limit",
                 "Waiting": "waiting", "Refused": "refused", "Cut": "cut", "Stopped": "stopped"}
        self.assertEqual(set(arms), set(names))
        for variant, code in arms.items():
            self.assertEqual(tb.ended(int(code)), names[variant], variant)
        self.assertEqual((tb.ended(130), tb.ended(143), tb.ended(None)),
                         ("signalled", "signalled", "unknown"))


# A stand-in for `theseus`: `ask` reads the instruction, then answers as
# STANDIN_ASK says (done, failed, or wait: until a SIGTERM, then stopped);
# `history` prints a session with no nodes.
STANDIN = """#!/bin/sh
case "$*" in
  *" history"*) echo '{"session": {"session_id": "ses_invented"}, "nodes": []}' ;;
  *" ask "*)
    cat > "$STANDIN_DIR/instruction"
    case "$STANDIN_ASK" in
      done) echo '{"stop_reason": "no_tool_calls"}' ;;
      failed) echo "theseus: the provider is rate limited (rate_limit_error)" >&2; exit 1 ;;
      wait)
        trap 'echo "{\\"stop_reason\\": \\"stopped\\"}"; exit 9' TERM
        while :; do sleep 0.05; done ;;
    esac ;;
esac
"""


class Scripts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.bin, self.state, self.logs = d / "bin", d / "state", d / "logs"
        for p in (self.bin, self.state, self.logs):
            p.mkdir()
        for name in ("theseus", "theseusd"):
            (self.bin / name).write_text(STANDIN)
            (self.bin / name).chmod(stat.S_IRWXU)

    def tearDown(self):
        self.tmp.cleanup()

    def start(self, ask: str) -> subprocess.Popen:
        env = dict(os.environ, STANDIN_DIR=self.tmp.name, STANDIN_ASK=ask,
                   THESEUS_BENCH_INSTRUCTION="Fix the repository's history.")
        script = tb.run_script(str(self.bin), str(self.state), str(self.logs))
        return subprocess.Popen(["bash", "-c", "set -o pipefail; " + script], env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def read(self, name: str) -> str:
        return (self.logs / name).read_text()

    def test_a_turn_that_ends_leaves_its_result_history_and_code(self):
        p = self.start("done")
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertEqual(json.loads(self.read(tb.TURN))["stop_reason"], "no_tool_calls")
        self.assertEqual(json.loads(self.read(tb.HISTORY))["nodes"], [])
        self.assertEqual(self.read(tb.EXIT).strip(), "0")
        self.assertTrue((self.logs / tb.DONE).exists())
        instruction = (Path(self.tmp.name) / "instruction").read_text()
        self.assertEqual(instruction, "Fix the repository's history.")

    def test_a_failure_says_its_cause_on_stderr_where_harbor_reads_it(self):
        p = self.start("failed")
        _, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 1)
        self.assertIn("rate_limit_error", err)
        self.assertEqual(self.read(tb.EXIT).strip(), "1")
        self.assertTrue((self.logs / tb.HISTORY).exists())

    def test_the_stop_script_stops_the_turn_and_waits_for_the_runs_end(self):
        p = self.start("wait")
        deadline = time.monotonic() + 30
        while not (self.state / "ask.pid").exists() or not (Path(self.tmp.name) / "instruction").exists():
            self.assertLess(time.monotonic(), deadline, "the turn never started")
            time.sleep(0.02)
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs), 10)],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(stop.returncode, 0, stop.stderr)
        # The stop script returns once the run has read the history.
        self.assertTrue((self.logs / tb.DONE).exists())
        p.communicate(timeout=30)
        self.assertEqual(p.returncode, 9)
        self.assertEqual(json.loads(self.read(tb.TURN))["stop_reason"], "stopped")
        self.assertEqual(self.read(tb.EXIT).strip(), "9")

    def test_the_stop_script_with_no_turn_does_nothing(self):
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs), 1)],
                              capture_output=True, text=True, timeout=30)
        self.assertEqual(stop.returncode, 0, stop.stderr)


# An invented session: the user's message, a model answer with one call, the
# gate's decision, its result, and the closing answer.
HISTORY = {
    "session": {"session_id": "ses_invented1", "cost_usd": 0.03},
    "nodes": [
        {"kind": "user_message", "at_unix_ms": 1791067619162, "text": "Fix the repository."},
        {"kind": "assistant_message", "at_unix_ms": 1791067621537, "loop_index": 0, "text": "",
         "detail": {"model": "claude-sonnet-5-5", "provider": "anthropic", "stop_reason": "tool_use",
                    "cost_usd": 0.02,
                    "usage": {"input_tokens": 100, "output_tokens": 40,
                              "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 500},
                    "tool_calls": [{"id": "toolu_1", "name": "proc_run",
                                    "input": {"argv": ["git", "status"]}}]}},
        {"kind": "tool_call", "at_unix_ms": 1791067621563, "text": "",
         "detail": {"tool_use_id": "toolu_1", "tool": "proc.run",
                    "decision": {"posture": "open", "reason": "enforcement open"}}},
        {"kind": "tool_result", "at_unix_ms": 1791067621728, "text": "[exit code 0]\n## main\n",
         "detail": {"tool_use_id": "toolu_1", "tool": "proc.run", "status": "ok",
                    "is_error": False, "duration_ms": 153}},
        {"kind": "assistant_message", "at_unix_ms": 1791067623000, "loop_index": 1,
         "text": "The repository is fixed.",
         "detail": {"model": "claude-sonnet-5-5", "stop_reason": "end_turn", "cost_usd": 0.01,
                    "usage": {"input_tokens": 50, "output_tokens": 20,
                              "cache_read_input_tokens": 1500}}},
    ],
}


class Trajectories(unittest.TestCase):
    def trajectory(self, turn=None):
        return atif.trajectory(HISTORY, version="0.0.1", model_name="claude-sonnet-5-5", turn=turn)

    def test_each_answer_is_a_step_with_its_calls_results_and_metrics(self):
        t = self.trajectory(turn={"stop_reason": "no_tool_calls", "loops": 2, "tool_calls": 1})
        self.assertEqual([s["step_id"] for s in t["steps"]], [1, 2, 3])
        self.assertEqual([s["source"] for s in t["steps"]], ["user", "agent", "agent"])
        call = t["steps"][1]["tool_calls"][0]
        self.assertEqual((call["tool_call_id"], call["function_name"]), ("toolu_1", "proc_run"))
        self.assertEqual(call["arguments"], {"argv": ["git", "status"]})
        self.assertEqual(call["extra"]["posture"], "open")
        result = t["steps"][1]["observation"]["results"][0]
        self.assertEqual((result["source_call_id"], result["content"]),
                         ("toolu_1", "[exit code 0]\n## main\n"))
        self.assertEqual(t["steps"][1]["metrics"],
                         {"prompt_tokens": 1600, "completion_tokens": 40, "cached_tokens": 1000,
                          "cost_usd": 0.02, "extra": {"cache_creation_input_tokens": 500}})
        self.assertEqual(t["steps"][0]["timestamp"], "2026-10-03T22:46:59.162Z")
        self.assertEqual(t["final_metrics"],
                         {"total_prompt_tokens": 3150, "total_completion_tokens": 60,
                          "total_cached_tokens": 2500, "total_cost_usd": 0.03, "total_steps": 3})
        self.assertEqual(t["extra"], {"stop_reason": "no_tool_calls", "loops": 2, "tool_calls": 1})
        self.assertEqual(t["session_id"], "ses_invented1")

    def test_a_turn_cut_short_keeps_the_spend_of_its_finished_calls(self):
        s = atif.spend(HISTORY)
        self.assertEqual(s, {"cost_usd": 0.03, "input_tokens": 3150, "cache_tokens": 2500,
                             "output_tokens": 60, "calls": 2})
        unpriced = json.loads(json.dumps(HISTORY))
        del unpriced["nodes"][4]["detail"]["cost_usd"]
        self.assertIsNone(atif.spend(unpriced)["cost_usd"])

    def test_a_call_a_stop_cut_is_a_last_step_so_the_totals_are_the_turns(self):
        """A timeout's stop cut a third call before it answered: the turn's
        result counts it at the kernel's estimate, and no node holds it."""
        turn = {"stop_reason": "stopped", "model": "claude-sonnet-5-5", "cost_usd": 0.046,
                "usage": {"input_tokens": 150 + 8066, "output_tokens": 60,
                          "cache_read_input_tokens": 2500, "cache_creation_input_tokens": 500}}
        t = self.trajectory(turn=turn)
        last = t["steps"][-1]
        self.assertEqual((len(t["steps"]), last["source"], last["message"]), (4, "agent", ""))
        self.assertIn("unanswered", last["extra"])
        self.assertEqual(last["metrics"], {"prompt_tokens": 8066, "completion_tokens": 0,
                                           "cached_tokens": 0, "cost_usd": 0.016})
        self.assertEqual((t["final_metrics"]["total_cost_usd"], t["final_metrics"]["total_prompt_tokens"]),
                         (0.046, 3150 + 8066))
        # A turn that its answers account for gets no such step.
        whole = dict(turn, cost_usd=0.03, usage={"input_tokens": 150, "output_tokens": 60,
                                                  "cache_read_input_tokens": 2500,
                                                  "cache_creation_input_tokens": 500})
        self.assertEqual(len(self.trajectory(turn=whole)["steps"]), 3)

    def test_a_result_with_no_call_in_the_history_is_a_system_step(self):
        h = {"session": {}, "nodes": [HISTORY["nodes"][3]]}
        t = atif.trajectory(h, version="0.0.1")
        self.assertEqual([s["source"] for s in t["steps"]], ["system"])
        result = t["steps"][0]["observation"]["results"][0]
        self.assertEqual((result.get("source_call_id"), result["extra"]["tool_use_id"]), (None, "toolu_1"))
        empty = atif.trajectory({"session": {}, "nodes": []}, version="0.0.1")
        self.assertEqual(len(empty["steps"]), 1)

    @unittest.skipIf(Trajectory is None, "Harbor is not installed")
    def test_harbor_reads_it_as_atif_with_the_same_usage(self):
        for t in (self.trajectory(turn={"stop_reason": "no_tool_calls"}),
                  atif.trajectory({"session": {}, "nodes": [HISTORY["nodes"][3]]}, version="x")):
            parsed = Trajectory.model_validate_json(json.dumps(t))
            self.assertEqual(parsed.to_json_dict(), t)
        usage = compute_model_usage(Trajectory.model_validate(self.trajectory()))
        u = usage["claude-sonnet-5-5"]
        self.assertEqual((u.n_input_tokens, u.n_cache_tokens, u.n_output_tokens),
                         (3150, 2500, 60))
        self.assertAlmostEqual(u.cost_usd, 0.03)


@unittest.skipIf(Trajectory is None, "Harbor is not installed")
class Adapter(unittest.TestCase):
    def test_it_loads_as_an_atif_agent_with_an_error_for_each_end(self):
        import theseus_agent

        self.assertEqual(theseus_agent.Theseus.name(), "theseus")
        self.assertTrue(theseus_agent.Theseus.capabilities.atif)
        ends = {code for code, end in tb.ENDS.items() if end not in ("done", "failed", "usage", "unreachable")}
        self.assertEqual(set(theseus_agent.ERRORS), ends)


if __name__ == "__main__":
    unittest.main()

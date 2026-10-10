"""The scorer's Layer 2 columns (theseus-2wxa): overlap, calls per response
and the multi-call share, and order violations, over fixture trials worked by
hand, one trajectory per harness format.

    python3 -m unittest discover -s bench/async

Theseus's trajectory comes from its own converter (bench/harbor/theseus_atif.py)
over a fixture history, Pi's from its converter (pi_atif.py) over a fixture
session log, Claude Code's in the shape Harbor's converter writes (one step a
message id, its blocks' calls bundled; under ASYNC_HARBOR=1, from Harbor's
converter itself over a fixture session), and OpenCode's from its own stream
when no trajectory was written (and under ASYNC_HARBOR=1, from Harbor's).
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import pi_atif  # noqa: E402
import score  # noqa: E402
import theseus_atif  # noqa: E402
from test_score import start, trial  # noqa: E402

CALLS_EACH = [2, 0, 3, 1]  # each arm's fixture: four responses, two of them with several calls


def theseus_history(counts: list[int]) -> dict:
    nodes, at = [{"kind": "user_message", "text": "go", "at_unix_ms": 1_000_000}], 1_000_000
    for i, n in enumerate(counts):
        at += 1000
        calls = [{"id": f"tu_{i}_{j}", "name": "proc_run", "input": {"argv": ["host-check"]}} for j in range(n)]
        nodes.append({"kind": "assistant_message", "text": "" if n else "Done.", "at_unix_ms": at,
                      "detail": {"model": "claude-sonnet-5-5", "tool_calls": calls}})
        nodes += [{"kind": "tool_result", "text": "ok", "at_unix_ms": at + 1,
                   "detail": {"tool_use_id": c["id"], "tool": "proc.run"}} for c in calls]
    return {"session": {"session_id": "s"}, "nodes": nodes}


def pi_entries(counts: list[int]) -> list[dict]:
    out = [{"type": "session", "id": "pi-s"}, {"type": "message", "id": "u", "timestamp": "2026-10-10T10:00:00Z",
                                                "message": {"role": "user", "content": "go"}}]
    for i, n in enumerate(counts):
        content = [{"type": "toolCall", "id": f"c{i}{j}", "name": "bash", "arguments": {"command": "x"}}
                   for j in range(n)] or [{"type": "text", "text": "Done."}]
        out.append({"type": "message", "id": f"a{i}", "timestamp": f"2026-10-10T10:00:0{i + 1}Z",
                    "message": {"role": "assistant", "content": content}})
    return out


def claude_code_atif(counts: list[int]) -> dict:
    """Harbor 0.23's Claude Code converter: a step per message id, the calls
    of the stream's per-block entries bundled into it."""
    steps = [{"step_id": 1, "source": "user", "message": "go"}]
    for i, n in enumerate(counts):
        steps.append({"step_id": i + 2, "source": "agent", "message": "" if n else "Done.",
                      "tool_calls": [{"tool_call_id": f"t{i}{j}", "function_name": "Bash",
                                      "arguments": {"command": "x"}} for j in range(n)] or None})
    return {"schema_version": "ATIF-v1.7", "steps": steps}


def opencode_stream(counts: list[int]) -> str:
    events = []
    for i, n in enumerate(counts):
        events.append({"type": "step_start", "timestamp": 1000 * i, "part": {"type": "step-start"}})
        events += [{"type": "tool_use", "timestamp": 1000 * i + 1,
                    "part": {"type": "tool", "tool": "bash", "callID": f"c{i}{j}",
                             "state": {"status": "completed", "input": {"command": "x"}, "output": "ok"}}}
                   for j in range(n)]
        if not n:
            events.append({"type": "text", "timestamp": 1000 * i + 1, "part": {"type": "text", "text": "Done."}})
        events.append({"type": "step_finish", "timestamp": 1000 * i + 2,
                       "part": {"type": "step-finish", "cost": 0.001, "tokens": {"input": 1, "output": 1}}})
    return "\n".join(json.dumps(e) for e in events) + "\n"


# A host-logs ledger: six fetches, three at once and three in turn, the
# slow phase 100 s; and a pipeline whose run-app started while it built.
HOSTS = [start("fetch-log", "web-1", 1, 1010.0, 20.0), start("fetch-log", "web-2", 2, 1010.0, 20.0),
         start("fetch-log", "web-3", 3, 1010.0, 20.0),
         ("end", "fetch-log", "web-1", 1, 1030.0, {"errors": 0}), ("end", "fetch-log", "web-2", 2, 1030.0, {"errors": 2}),
         ("end", "fetch-log", "web-3", 3, 1030.0, {"errors": 0}),
         start("fetch-log", "web-4", 4, 1040.0, 20.0), ("end", "fetch-log", "web-4", 4, 1060.0, {"errors": 0}),
         start("fetch-log", "web-5", 5, 1060.0, 20.0), ("end", "fetch-log", "web-5", 5, 1080.0, {"errors": 0}),
         start("fetch-log", "web-6", 6, 1090.0, 20.0), ("end", "fetch-log", "web-6", 6, 1110.0, {"errors": 0})]
PIPELINE = [start("build", "build", 1, 1010.0, 20.0),
            start("run-app", "run", 2, 1012.0, 12.0, build="stale"),
            ("end", "run-app", "run", 2, 1024.0, {"output": "o1", "build": "stale"}),
            ("end", "build", "build", 1, 1030.0, {"build": "b1"}),
            start("run-app", "run", 3, 1031.0, 12.0, build="b1"),
            ("end", "run-app", "run", 3, 1043.0, {"output": "o2", "build": "b1"}),
            start("check-output", "check", 4, 1044.0, 11.0, output="o2"),
            ("end", "check-output", "check", 4, 1055.0, {"verdict": "v", "output": "o2"})]


class Columns(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        cls.jobs = []
        for arm, write in (("theseus-async", cls.theseus), ("pi-async", cls.pi),
                           ("claude-code-async", cls.claude), ("opencode", cls.opencode)):
            job = root / arm
            job.mkdir()
            trial(job, "host-logs__a", arm=arm, family="host-logs", start=1000.0, end=1120.0, reward=1.0,
                  events=HOSTS)
            trial(job, "pipeline__b", arm=arm, family="pipeline", start=1000.0, end=1060.0, reward=0.0,
                  events=PIPELINE, problems=["order violation: run-app run started (seq 1) before build ended"])
            for t in ("host-logs__a", "pipeline__b"):
                write(job / t / "agent")
            cls.jobs.append(job)
        cls.scores = [score.score(t) for t in score.trials(cls.jobs)]
        cls.by = {(s["arm"], s["family"]): s for s in cls.scores}

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    @staticmethod
    def theseus(agent: Path) -> None:
        traj = theseus_atif.trajectory(theseus_history(CALLS_EACH), version="0.0.1")
        (agent / "trajectory.json").write_text(json.dumps(traj))

    @staticmethod
    def pi(agent: Path) -> None:
        traj = pi_atif.trajectory(pi_entries(CALLS_EACH), version="1.0.4")
        (agent / "trajectory.json").write_text(json.dumps(traj))

    @staticmethod
    def claude(agent: Path) -> None:
        (agent / "trajectory.json").write_text(json.dumps(claude_code_atif(CALLS_EACH)))

    @staticmethod
    def opencode(agent: Path) -> None:
        (agent / "opencode.txt").write_text(opencode_stream(CALLS_EACH))

    def test_overlap_is_the_slow_steps_over_the_slow_phase(self):
        """Six 20 s fetches, 120 s of work, in a phase from 1010 to 1110:
        1.2; the agent's wall (120 s) and the time before the first fetch are
        not the phase."""
        s = self.by[("theseus-async", "host-logs")]
        self.assertEqual((s["slow_busy_s"], s["slow_phase_s"], s["overlap"]), (120.0, 100.0, 1.2))
        # A control's two runs of run-app, and its chain: 55 s of work in 45 s.
        p = self.by[("theseus-async", "pipeline")]
        self.assertEqual((p["slow_busy_s"], p["slow_phase_s"], p["overlap"]), (55.0, 45.0, 1.222))

    def test_calls_per_response_read_alike_from_every_harness_format(self):
        for arm in ("theseus-async", "pi-async", "claude-code-async", "opencode"):
            with self.subTest(arm=arm):
                s = self.by[(arm, "host-logs")]
                self.assertEqual((s["responses"], s["tool_responses"], s["tool_calls"]), (4, 3, 6), s)
                self.assertEqual((s["calls_per_response"], s["multi_call_share"], s["max_calls"]),
                                 (2.0, 0.667, 3))

    def test_order_violations_count_a_dependent_step_started_early(self):
        for arm in ("theseus-async", "opencode"):
            self.assertEqual(self.by[(arm, "pipeline")]["order_violations"], 1)
            # A family with no order rules has no such count.
            self.assertIsNone(self.by[(arm, "host-logs")]["order_violations"])

    def test_the_ideal_and_the_rows(self):
        self.assertEqual(self.by[("pi-async", "host-logs")]["ideal_s"], 20.0)
        self.assertEqual(self.by[("pi-async", "pipeline")]["ideal_s"], 43.0)
        rows = {(r["arm"], r["family"]): r for r in score.rows(self.scores)}
        r = rows[("opencode", "pipeline")]
        self.assertEqual((r["order_violations"], r["calls_per_response"], r["multi_call_share"], r["overlap"]),
                         (1, 2.0, 0.667, 1.222))
        md = score.markdown(score.rows(self.scores), ["x"])
        self.assertIn("| Overlap | Calls / response | Multi-call share | Order violations |", md)

    def test_a_label_names_the_arm_of_its_jobs_trials(self):
        with tempfile.TemporaryDirectory() as d:
            score.main([f"theseus-before={self.jobs[0]}", f"theseus-after={self.jobs[0]}", str(self.jobs[1]),
                        "--out", d])
            got = json.loads((Path(d) / "scores.json").read_text())
            self.assertEqual(sorted({s["arm"] for s in got}), ["pi-async", "theseus-after", "theseus-before"])

    def test_a_trial_with_no_trajectory_has_no_batching_numbers(self):
        self.assertEqual(score.batching(None)["calls_per_response"], None)
        self.assertEqual(score.batching([0, 0])["multi_call_share"], None)
        self.assertEqual(score.batching([1, 1, 4])["multi_call_share"], 0.333)


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class HarborConverters(unittest.TestCase):
    """Harbor's own converters give one agent step a response."""

    def test_claude_codes_per_block_entries_are_one_response(self):
        from harbor.agents.installed.claude_code import ClaudeCode

        with tempfile.TemporaryDirectory() as d:
            sess = Path(d) / "s1"
            sess.mkdir()
            lines = [{"type": "user", "uuid": "u0", "sessionId": "s1", "timestamp": "2026-10-10T10:00:00.000Z",
                      "message": {"role": "user", "content": "go"}}]
            t = 0
            for i, n in enumerate(CALLS_EACH):
                blocks = [{"type": "tool_use", "id": f"t{i}{j}", "name": "Bash", "input": {"command": "x"}}
                          for j in range(n)] or [{"type": "text", "text": "Done."}]
                for j, b in enumerate(blocks):
                    t += 1
                    lines.append({"type": "assistant", "uuid": f"a{i}{j}", "sessionId": "s1",
                                  "timestamp": f"2026-10-10T10:00:{t:02d}.000Z",
                                  "message": {"id": f"msg_{i}", "role": "assistant", "model": "claude-sonnet-5-5",
                                              "content": [b], "usage": {"input_tokens": 1, "output_tokens": 1}}})
            (sess / "s1.jsonl").write_text("\n".join(json.dumps(x) for x in lines) + "\n")
            traj = ClaudeCode(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5") \
                ._convert_events_to_trajectory(sess).to_json_dict()
            agent = Path(d) / "trial/agent"
            agent.mkdir(parents=True)
            (agent / "trajectory.json").write_text(json.dumps(traj))
            self.assertEqual(score.responses(agent.parent), CALLS_EACH)

    def test_opencodes_steps_are_its_responses(self):
        from harbor.agents.installed.opencode import OpenCode

        with tempfile.TemporaryDirectory() as d:
            oc = OpenCode(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
            events = [json.loads(line) for line in opencode_stream(CALLS_EACH).splitlines()]
            traj = oc._convert_events_to_trajectory(events).to_json_dict()
            agent = Path(d) / "trial/agent"
            agent.mkdir(parents=True)
            (agent / "trajectory.json").write_text(json.dumps(traj))
            self.assertEqual(score.responses(agent.parent), CALLS_EACH)


if __name__ == "__main__":
    unittest.main()

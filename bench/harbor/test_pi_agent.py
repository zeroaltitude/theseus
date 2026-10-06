"""Tests for the measured Pi arm (theseus-jp9p).

    python3 -m unittest discover -s bench/harbor                 # the record and the trajectory
    .venv/bin/python -m unittest discover -s bench/harbor         # with Harbor: the agent itself

The record and the trajectory are read from a fixture of Pi's session log
(its version-3 JSONL, in the shape Pi 1.0.4 writes; the content invented), with
the numbers worked by hand. The agent's checks need Harbor and skip on the
standard library.
"""

from __future__ import annotations

import asyncio
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import efficiency as ef
import pi_atif

try:
    from harbor.agents.installed.pi import Pi
    from harbor.environments.base import ExecResult
    from harbor.models.agent.context import AgentContext
    from harbor.models.trajectories import Trajectory
    from harbor.utils.trajectory_utils import compute_model_usage

    import pi_agent as pa
except ImportError:  # Harbor is not installed: these skip.
    Pi = None

SONNET, FALLBACK = "claude-sonnet-5-5", "claude-sonnet-5"


def usage(i, r, w, o, cost):
    return {"input": i, "output": o, "cacheRead": r, "cacheWrite": w, "totalTokens": i + r + w + o,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": cost}}


def entry(eid, parent, at, **rest):
    return {"id": eid, "parentId": parent, "timestamp": f"2026-10-06T12:00:{at:02d}.000Z", **rest}


def answer(eid, parent, at, model, u, *, text="", calls=(), stop="toolUse", error=None):
    content = ([{"type": "thinking", "thinking": "Look first."}] if calls else []) + \
        ([{"type": "text", "text": text}] if text else []) + \
        [{"type": "toolCall", "id": c, "name": n, "arguments": a} for c, n, a in calls]
    msg = {"role": "assistant", "content": content, "api": "anthropic-messages", "provider": "anthropic",
           "model": model, "usage": u, "stopReason": stop, "timestamp": 0}
    if error:
        msg["errorMessage"] = error
    return entry(eid, parent, at, type="message", message=msg)


def result(eid, parent, at, call, tool, text, u=None):
    msg = {"role": "toolResult", "toolCallId": call, "toolName": tool,
           "content": [{"type": "text", "text": text}], "isError": False, "timestamp": 0}
    if u is not None:
        msg["usage"] = u
    return entry(eid, parent, at, type="message", message=msg)


A1 = answer("a1", "u1", 2, SONNET, usage(120, 0, 1800, 45, 0.00519), text="I will look.",
            calls=[("toolu_1", "bash", {"command": "git status"})])
# A retry's failed request, persisted as Pi persists it: no usage, an error.
A2 = answer("a2", "r1", 4, SONNET, usage(0, 0, 0, 0, 0), stop="error", error="529 overloaded")
A3 = answer("a3", "a2", 6, SONNET, usage(60, 1800, 200, 30, 0.00128),
            calls=[("toolu_2", "write", {"path": "notes.txt", "content": "plover"}),
                   ("toolu_3", "bash", {"command": "make check"})])
# A summary's own call: the entry names no model, so it is the session's.
COMPACTION = entry("c1", "r3", 9, type="compaction", summary="The repository was checked.",
                   firstKeptEntryId="a3", tokensBefore=9000, usage=usage(3000, 0, 0, 400, 0.01))
A4 = answer("a4", "c1", 11, FALLBACK, usage(20, 0, 2000, 12, 0.0062), text="Done.", stop="stop")

SESSION = [
    {"type": "session", "version": 3, "id": "01a11296-0000-7000-8000-00000000beef",
     "timestamp": "2026-10-06T12:00:00.000Z", "cwd": "/app"},
    entry("m1", None, 0, type="model_change", provider="anthropic", modelId=SONNET),
    entry("t1", "m1", 0, type="thinking_level_change", thinkingLevel="medium"),
    entry("s1", "t1", 1, type="message", message={"role": "system", "content": "", "sections": {
        "preamble": "You are a coding assistant."}, "toolsAdded": [{"name": "bash"}], "timestamp": 0}),
    entry("u1", "s1", 1, type="message", message={"role": "user", "content": [
        {"type": "text", "text": "Fix the repository."}], "timestamp": 0}),
    A1,
    result("r1", "a1", 3, "toolu_1", "bash", "On branch main"),
    A2,
    A3,
    result("r2", "a3", 7, "toolu_2", "write", "wrote notes.txt"),
    # A tool's own model work: tokens and dollars, and no call of the turn's.
    result("r3", "r2", 8, "toolu_3", "bash", "ok", usage(10, 0, 0, 5, 0.0001)),
    COMPACTION,
    A4,
    A1,  # the same entry again: read once, by its id
]

# Worked by hand.
TOKENS = {"input": 3210, "cache_read": 1800, "cache_write": 4000, "output": 492}
BY_MODEL = {
    SONNET: {"input": 3190, "cache_read": 1800, "cache_write": 2000, "output": 480,
             "cost_usd": 0.01657, "calls": 4},
    FALLBACK: {"input": 20, "cache_read": 0, "cache_write": 2000, "output": 12, "cost_usd": 0.0062, "calls": 1},
}
COST = 0.02277


def stream_of(entries):
    """Pi's `--mode json` stream as Harbor tees it to pi.txt (no
    `message_update` lines), with a line of stderr in it."""
    lines = ['{"type":"session","version":3,"id":"x"}', "Warning: an invented line of stderr"]
    seen = set()
    for e in entries:
        if e.get("id") in seen:
            continue
        seen.add(e.get("id"))
        if e.get("type") == "message":
            lines.append(json.dumps({"type": "message_start", "message": e["message"]}))
            lines.append(json.dumps({"type": "message_end", "message": e["message"]}))
        elif e.get("type") == "compaction":
            lines.append(json.dumps({"type": "compaction_end", "reason": "threshold", "result": {
                "summary": e["summary"], "usage": e["usage"]}, "aborted": False, "willRetry": False}))
    return "\n".join(lines) + "\n"


SUMMARY = {"status": "ok", "interval_ms": 250, "samples": 40, "wall_s": 10.0,
           "classes": {"harness": {"cpu_s": 0.9, "peak_rss_kb": 108000, "peak_hwm_kb": 110000, "processes": 1},
                       "work": {"cpu_s": 2.0, "peak_rss_kb": 7000, "peak_hwm_kb": 7000, "processes": 3},
                       "wrapper": {"cpu_s": 0.0, "peak_rss_kb": 0, "peak_hwm_kb": 0, "processes": 0},
                       "outside": {"cpu_s": 0.1}},
           "sampler": {"cpu_s": 0.05}}


def write_logs(logs: Path, entries=SESSION, stream: bool = True) -> None:
    d = logs / "pi" / "sessions"
    d.mkdir(parents=True, exist_ok=True)
    (d / "2026-10-06T12-00-00-000Z_01a11296.jsonl").write_text("\n".join(json.dumps(e) for e in entries) + "\n")
    if stream:
        (logs / "pi.txt").write_text(stream_of(entries))


class Record(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_arm_is_pis_cli_by_its_process_name(self):
        self.assertEqual(ef.ARMS["pi"], {"names": ("pi",), "wrapper_args": ()})

    def test_the_session_log_gives_every_class_model_and_call_once(self):
        s = ef.pi_spend(ef.read_jsonl([]) + SESSION)
        self.assertEqual(s["spend_from"], "session_log")
        self.assertEqual(s["tokens"], TOKENS)
        self.assertEqual(s["by_model"], BY_MODEL)
        self.assertEqual(s["cost_usd"], COST)
        # Four answers (the failed one a call of its own) and the summary's.
        self.assertEqual((s["model_calls"], s["tool_calls"]), (5, 3))

    def test_the_stream_is_the_spend_when_no_log_was_kept(self):
        s = ef.pi_spend([], stream_of(SESSION))
        self.assertEqual(s["spend_from"], "stream")
        self.assertEqual((s["tokens"], s["by_model"], s["cost_usd"], s["model_calls"], s["tool_calls"]),
                         (TOKENS, BY_MODEL, COST, 5, 3))

    def test_an_unpriced_call_leaves_the_dollars_unknown(self):
        bare = dict(A1, id="a9", message=dict(A1["message"], usage={"input": 5, "output": 1}))
        s = ef.pi_spend(SESSION + [bare])
        self.assertIsNone(s["cost_usd"])
        self.assertIsNone(s["by_model"][SONNET]["cost_usd"])

    def test_the_record_from_the_agent_directory(self):
        write_logs(self.logs)
        (self.logs / "sampler.json").write_text(json.dumps(SUMMARY))
        rec = ef.pi_record(self.logs, max_budget_usd=2.0, max_turns=200)
        self.assertEqual((rec["schema"], rec["arm"], rec["spend_from"]), (ef.SCHEMA, "pi", "session_log"))
        self.assertEqual((rec["tokens"], rec["cost_usd"], rec["model_calls"]), (TOKENS, COST, 5))
        self.assertEqual((rec["harness"]["cpu_s"], rec["wall_s"], rec["wall_from"]), (0.9, 10.0, "sampler"))
        self.assertEqual(rec["end"], {"stop_reason": "stop", "error": None, "answers": 4})
        self.assertEqual(rec["limits"], {"enforced": False, "max_budget_usd": 2.0, "max_turns": 200,
                                         "answers": 4, "over_budget": False, "over_turns": False})

    def test_without_the_sampler_the_wall_is_the_logs_span(self):
        write_logs(self.logs, stream=False)
        rec = ef.pi_record(self.logs)
        self.assertEqual((rec["wall_s"], rec["wall_from"]), (11.0, "agent"))
        self.assertEqual(rec["sampler"]["status"], "missing")

    def test_a_run_that_ended_on_a_provider_error_says_so(self):
        write_logs(self.logs, SESSION[:8])
        rec = ef.pi_record(self.logs)
        self.assertEqual(rec["end"], {"stop_reason": "error", "error": "529 overloaded", "answers": 2})

    def test_a_trial_past_the_others_caps_is_flagged_not_stopped(self):
        write_logs(self.logs)
        rec = ef.pi_record(self.logs, max_budget_usd=0.02, max_turns=3)
        self.assertEqual((rec["limits"]["over_budget"], rec["limits"]["over_turns"]), (True, True))
        rec = ef.pi_record(self.logs, max_budget_usd=0.03, max_turns=4)
        self.assertEqual((rec["limits"]["over_budget"], rec["limits"]["over_turns"]), (False, False))
        rec = ef.pi_record(self.logs)
        self.assertEqual((rec["limits"]["over_budget"], rec["limits"]["over_turns"]), (None, None))
        self.assertFalse(rec["limits"]["enforced"])

    def test_an_empty_directory_is_a_record_of_nothing(self):
        rec = ef.pi_record(self.logs)
        self.assertEqual((rec["spend_from"], rec["cost_usd"], rec["model_calls"]), (None, None, None))
        self.assertEqual(rec["limits"]["answers"], None)


class Atif(unittest.TestCase):
    def trajectory(self, entries=SESSION):
        return pi_atif.trajectory(entries, version="1.0.4", model_name="anthropic/claude-sonnet-5-5")

    def test_its_steps(self):
        t = self.trajectory()
        self.assertEqual((t["schema_version"], t["session_id"], t["agent"]),
                         ("ATIF-v1.7", "01a11296-0000-7000-8000-00000000beef",
                          {"name": "pi", "version": "1.0.4", "model_name": "anthropic/claude-sonnet-5-5"}))
        steps = t["steps"]
        self.assertEqual([s["step_id"] for s in steps], list(range(1, 8)))
        self.assertEqual([s["source"] for s in steps], ["user"] + ["agent"] * 6)
        self.assertEqual(steps[0]["message"], "Fix the repository.")
        first = steps[1]
        self.assertEqual((first["message"], first["reasoning_content"], first["model_name"]),
                         ("I will look.", "Look first.", SONNET))
        self.assertEqual(first["tool_calls"], [{"tool_call_id": "toolu_1", "function_name": "bash",
                                                "arguments": {"command": "git status"}}])
        self.assertEqual(first["observation"]["results"][0]["source_call_id"], "toolu_1")
        self.assertEqual(first["metrics"], {"prompt_tokens": 1920, "completion_tokens": 45, "cached_tokens": 0,
                                            "cost_usd": 0.00519,
                                            "extra": {"cache_creation_input_tokens": 1800}})
        self.assertEqual(steps[2]["extra"]["error"], "529 overloaded")
        self.assertEqual([r["source_call_id"] for r in steps[3]["observation"]["results"]],
                         ["toolu_2", "toolu_3"])
        self.assertEqual(steps[4]["extra"], {"nested": "bash"})
        self.assertEqual((steps[5]["message"], steps[5]["extra"], steps[5]["model_name"]),
                         ("The repository was checked.", {"kind": "compaction"}, SONNET))
        self.assertEqual(steps[6]["model_name"], FALLBACK)

    def test_its_totals_are_the_records(self):
        final = self.trajectory()["final_metrics"]
        self.assertEqual(final, {"total_prompt_tokens": sum(TOKENS[c] for c in ("input", "cache_read",
                                                                                 "cache_write")),
                                 "total_completion_tokens": TOKENS["output"],
                                 "total_cached_tokens": TOKENS["cache_read"], "total_steps": 7,
                                 "total_cost_usd": COST})
        self.assertEqual(ef.trajectory_spend(self.trajectory())["tokens"], TOKENS)

    def test_a_result_with_no_call_in_the_log_is_a_system_step(self):
        t = self.trajectory([SESSION[0], SESSION[6]])
        self.assertEqual([s["source"] for s in t["steps"]], ["system"])
        r = t["steps"][0]["observation"]["results"][0]
        self.assertEqual((r.get("source_call_id"), r["extra"]["tool_call_id"]), (None, "toolu_1"))
        self.assertEqual(len(self.trajectory([])["steps"]), 1)

    @unittest.skipIf(Pi is None, "Harbor is not installed")
    def test_harbor_reads_it_as_atif_with_the_same_usage(self):
        t = self.trajectory()
        parsed = Trajectory.model_validate_json(json.dumps(t))
        self.assertEqual(parsed.to_json_dict(), t)
        u = compute_model_usage(parsed)
        self.assertEqual((u[SONNET].n_input_tokens, u[SONNET].n_cache_tokens, u[SONNET].n_output_tokens),
                         (3190 + 1800 + 2000, 1800, 480))
        self.assertAlmostEqual(u[SONNET].cost_usd + u[FALLBACK].cost_usd, COST)


class FakeEnvironment:
    """What the arm asks of a container: each command, its environment, and
    its user, as sent."""

    default_user = "agent"

    def __init__(self):
        self.commands: list[str] = []
        self.envs: list[dict | None] = []
        self.uploads: list[tuple[str, str]] = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        self.commands.append(command)
        self.envs.append(env)
        return ExecResult(stdout="", stderr="", return_code=0)

    async def upload_file(self, source_path, target_path):
        self.uploads.append((str(source_path), target_path))


# Hermetic: the key Harbor passes in, and no endpoint of the host's.
HOST = {"ANTHROPIC_API_KEY": "sk-ant-invented"}


@unittest.skipIf(Pi is None, "Harbor is not installed")
class Arm(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)
        env = {k: v for k, v in os.environ.items() if not k.startswith(("ANTHROPIC_", "CLAUDE_"))}
        self.env = mock.patch.dict(os.environ, {**env, **HOST}, clear=True)
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def agent(self, **kw):
        return pa.MeasuredPi(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5", **kw)

    def test_it_is_harbors_pi_by_name_with_the_others_caps_as_options(self):
        self.assertTrue(issubclass(pa.MeasuredPi, Pi))
        self.assertEqual(pa.MeasuredPi.name(), Pi.name())
        self.assertTrue(pa.MeasuredPi.capabilities.atif)
        self.assertEqual(pa.MeasuredPi.capabilities.resume, Pi.capabilities.resume)
        a = self.agent(max_budget_usd=2.0, max_turns=200, thinking="off")
        self.assertEqual((a.options.max_budget_usd, a.options.max_turns), (2.0, 200))
        # Pi has no such flags: the caps never reach its command line.
        self.assertEqual(a.build_cli_flags(), "--thinking off")

    def test_the_install_is_pinned(self):
        env = FakeEnvironment()
        asyncio.run(self.agent().install(env))
        npm = [c for c in env.commands if "npm install -g" in c]
        self.assertEqual(len(npm), 1)
        self.assertIn(f"@earendil-works/pi-coding-agent@{pa.PINNED_VERSION} ", npm[0])
        self.assertNotIn("@latest", npm[0])
        self.assertEqual(pa.PINNED_VERSION, "1.0.4")
        # The sampler goes up after Pi.
        self.assertEqual(env.uploads, [(str(Path(pa.smp.__file__)), pa.SAMPLER)])
        self.assertIn("chown agent", env.commands[-1])
        # A version named for the trial wins over the pin.
        self.assertEqual(self.agent(version="1.0.3").version(), "1.0.3")

    def test_the_command_line_model_and_key(self):
        env = FakeEnvironment()
        asyncio.run(self.agent(max_turns=200, thinking="high").run("Fix the repository.", env, AgentContext()))
        runs = [(c, e) for c, e in zip(env.commands, env.envs) if " pi --print" in c]
        self.assertEqual(len(runs), 1)
        command, run_env = runs[0]
        self.assertIn("pi --print --mode json --session-dir /logs/agent/pi/sessions "
                      "--provider anthropic --model claude-sonnet-5-5 --thinking high "
                      "'Fix the repository.'", command)
        self.assertIn("tee /logs/agent/pi.txt", command)
        self.assertNotIn("max", command.split("'Fix")[0])
        self.assertEqual(run_env, {"ANTHROPIC_API_KEY": "sk-ant-invented"})

    def test_the_sampler_runs_around_harbors_run(self):
        seen = []

        async def harbors(self_, instruction, environment, context):
            seen.append((instruction, list(environment.commands)))

        env = FakeEnvironment()
        with mock.patch.object(Pi, "run", harbors):
            asyncio.run(self.agent().run("Fix the repository.", env, AgentContext()))
        self.assertEqual(seen[0][0], "Fix the repository.")
        self.assertEqual(len(seen[0][1]), 1)
        self.assertIn(f"python3 {pa.SAMPLER} --out /logs/agent --names pi", seen[0][1][0])
        self.assertEqual(env.commands[-1], pa.smp.stop_script("/logs/agent", pa.STATE))

    def test_harbors_timeout_still_stops_the_sampler(self):
        async def cancelled(self_, instruction, environment, context):
            environment.commands.append("pi, cancelled")
            raise asyncio.CancelledError

        env = FakeEnvironment()
        with mock.patch.object(Pi, "run", cancelled):
            with self.assertRaises(asyncio.CancelledError):
                asyncio.run(self.agent().run("Fix it.", env, AgentContext()))
        self.assertEqual(env.commands[-2:], ["pi, cancelled", pa.smp.stop_script("/logs/agent", pa.STATE)])

    def test_the_trajectory_record_and_counters_follow_harbors_own(self):
        write_logs(self.logs)
        (self.logs / "sampler.json").write_text(json.dumps(SUMMARY))
        ctx = AgentContext()
        self.agent(max_budget_usd=0.02, max_turns=200).populate_context_post_run(ctx)
        traj = json.loads((self.logs / "trajectory.json").read_text())
        Trajectory.model_validate(traj)
        self.assertEqual((traj["agent"]["name"], traj["agent"]["version"]), ("pi", "1.0.4"))
        rec = json.loads((self.logs / ef.RECORD).read_text())
        self.assertEqual(ctx.metadata["efficiency"], rec)
        self.assertEqual((rec["arm"], rec["tokens"], rec["cost_usd"]), ("pi", TOKENS, COST))
        self.assertEqual((rec["limits"]["over_budget"], rec["harness"]["cpu_s"]), (True, 0.9))
        # The cache write is input, as the other arms' counters hold it.
        self.assertEqual((ctx.n_input_tokens, ctx.n_cache_tokens, ctx.n_output_tokens),
                         (3210 + 1800 + 4000, 1800, 492))
        self.assertEqual(ctx.cost_usd, COST)

    def test_no_logs_is_a_record_and_no_failure(self):
        ctx = AgentContext()
        self.agent().populate_context_post_run(ctx)
        self.assertFalse((self.logs / "trajectory.json").exists())
        self.assertEqual(ctx.metadata["efficiency"]["arm"], "pi")
        self.assertIsNone(ctx.cost_usd)


if __name__ == "__main__":
    unittest.main()

"""Tests for the trial's efficiency record (theseus-7gir.12).

    python3 -m unittest discover -s bench/harbor

Fixture turns, histories, session logs, and sampler summaries, with the
numbers worked by hand.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import efficiency as ef


def answer(model, usage, cost, *, turn="turn_1", calls=(), stop="tool_use"):
    return {"kind": "assistant_message", "turn_id": turn, "text": "",
            "detail": {"model": model, "stop_reason": stop, "usage": usage, "cost_usd": cost,
                       "tool_calls": [{"id": c, "name": "proc_run", "input": {}} for c in calls]}}


def tool_call(cid):
    return {"kind": "tool_call", "turn_id": "turn_1", "detail": {"tool_use_id": cid, "tool": "proc.run"}}


def usage(i, r, w, o):
    return {"input_tokens": i, "cache_read_input_tokens": r, "cache_creation_input_tokens": w,
            "output_tokens": o}


def trace(*provider_calls):
    """A turn > loop > provider trace, one loop per entry."""
    return {"kind": "turn", "name": "turn", "children": [
        {"kind": "loop", "name": "loop", "children": [{"kind": "provider", "name": "provider.call"}] * n}
        for n in provider_calls]}


SONNET, FALLBACK = "claude-sonnet-5-5", "claude-sonnet-5"

PLAIN_HISTORY = {"session": {"session_id": "ses_invented1"}, "nodes": [
    {"kind": "user_message", "text": "Fix the repository."},
    answer(SONNET, usage(100, 1000, 500, 40), 0.02, calls=["toolu_1"]),
    tool_call("toolu_1"),
    {"kind": "tool_result", "detail": {"tool_use_id": "toolu_1"}},
    answer(SONNET, usage(50, 1500, 0, 20), 0.01, stop="end_turn"),
]}
PLAIN_TURN = {"turn_id": "turn_1", "model": SONNET, "stop_reason": "no_tool_calls", "loops": 2,
              "tool_calls": 1, "elapsed_ms": 12500, "cost_usd": 0.03,
              "usage": usage(150, 2500, 500, 60), "trace": trace(1, 1)}


class Theseus(unittest.TestCase):
    def test_a_plain_turn_keeps_all_four_classes(self):
        s = ef.theseus_spend(PLAIN_TURN, PLAIN_HISTORY)
        self.assertEqual(s["tokens"], {"input": 150, "cache_read": 2500, "cache_write": 500,
                                       "output": 60})
        self.assertEqual(s["by_model"], {SONNET: {"input": 150, "cache_read": 2500,
                                                  "cache_write": 500, "output": 60,
                                                  "cost_usd": 0.03, "calls": 2}})
        self.assertEqual((s["cost_usd"], s["spend_from"], s["model_calls"], s["tool_calls"]),
                         (0.03, "turn", 2, 1))

    def test_a_cut_turn_is_its_historys_answers(self):
        """A timeout printed no result: the history's answers are the spend."""
        s = ef.theseus_spend(None, PLAIN_HISTORY)
        self.assertEqual(s["tokens"], {"input": 150, "cache_read": 2500, "cache_write": 500,
                                       "output": 60})
        self.assertEqual((s["cost_usd"], s["spend_from"], s["model_calls"], s["tool_calls"]),
                         (0.03, "history", 2, 1))
        unpriced = json.loads(json.dumps(PLAIN_HISTORY))
        del unpriced["nodes"][4]["detail"]["cost_usd"]
        self.assertIsNone(ef.theseus_spend(None, unpriced)["cost_usd"])
        self.assertEqual(ef.theseus_spend(None, None)["spend_from"], None)

    def test_a_trial_with_no_files_has_unknown_calls_not_zero(self):
        for s in (ef.theseus_spend(None, None), ef.theseus_spend({}, {}),
                  ef.claude_code_spend("", [], None)):
            self.assertIsNone(s["model_calls"])
            self.assertIsNone(s["tool_calls"])

    def test_a_fallback_and_a_retry_bill_by_model(self):
        """Sonnet 5.5 refused; the request went to Sonnet 5, whose first try
        failed (a transient error, retried: three provider calls, two
        answers). The failed try billed 300 input tokens at the kernel's
        estimate, which no answer holds: the turn's model (Sonnet 5) has it."""
        history = {"session": {}, "nodes": [
            {"kind": "user_message", "text": "Recover the hash."},
            answer(SONNET, usage(200, 0, 3000, 10), 0.012, stop="refusal"),
            answer(FALLBACK, usage(100, 3000, 0, 80), 0.004, stop="end_turn"),
        ]}
        turn = {"turn_id": "turn_1", "model": FALLBACK, "cost_usd": 0.017, "tool_calls": 0,
                "usage": usage(600, 3000, 3000, 90), "trace": trace(1, 2),
                "fallback": {"from": SONNET, "to": FALLBACK, "answered": True}}
        s = ef.theseus_spend(turn, history)
        self.assertEqual(s["tokens"], {"input": 600, "cache_read": 3000, "cache_write": 3000,
                                       "output": 90})
        self.assertEqual(s["by_model"][SONNET], {"input": 200, "cache_read": 0,
                                                 "cache_write": 3000, "output": 10,
                                                 "cost_usd": 0.012, "calls": 1})
        self.assertEqual(s["by_model"][FALLBACK], {"input": 400, "cache_read": 3000,
                                                   "cache_write": 0, "output": 80,
                                                   "cost_usd": 0.005, "calls": 1})
        self.assertEqual((s["cost_usd"], s["model_calls"]), (0.017, 3))

    def test_a_followed_run_is_every_turns_spend_once(self):
        """`ask` followed a late result (theseus-mqxk): one object, the last
        turn's result with both turns' spend summed, the ask's own turn in
        `asked` and the late result's in `continuations`. Both turns'
        answers are the run's, and none is counted twice."""
        history = json.loads(json.dumps(PLAIN_HISTORY))
        history["nodes"].append(answer(SONNET, usage(30, 2000, 0, 10), 0.005, turn="turn_2",
                                       stop="end_turn"))
        turn = dict(PLAIN_TURN, turn_id="turn_2", cost_usd=0.035, loops=3, trace=trace(1),
                    usage=usage(180, 4500, 500, 70),
                    asked={"turn_id": "turn_1", "cost_usd": 0.03},
                    continuations=[{"turn_id": "turn_2", "cost_usd": 0.005}])
        s = ef.theseus_spend(turn, history)
        self.assertEqual(s["tokens"], {"input": 180, "cache_read": 4500, "cache_write": 500,
                                       "output": 70})
        self.assertEqual(s["by_model"][SONNET]["calls"], 3)
        self.assertEqual((s["cost_usd"], s["model_calls"]), (0.035, 3))
        self.assertEqual(ef.run_turn_ids(turn), ("turn_2", "turn_1", "turn_2"))
        self.assertEqual(ef.run_turn_ids(PLAIN_TURN), ("turn_1",))
        self.assertIsNone(ef.run_turn_ids(None))

    def test_the_record_reads_a_trials_agent_directory(self):
        with tempfile.TemporaryDirectory() as t:
            logs = Path(t)
            (logs / "theseus-turn.json").write_text(json.dumps(PLAIN_TURN))
            (logs / "theseus-history.json").write_text(json.dumps(PLAIN_HISTORY))
            rec = ef.theseus_record(logs)
            self.assertEqual((rec["schema"], rec["arm"], rec["wall_s"], rec["wall_from"]),
                             (ef.SCHEMA, "theseus", 12.5, "agent"))
            self.assertEqual(rec["sampler"], {"status": "missing", "reason": "no sampler.json"})
            self.assertIsNone(rec["harness"])
            (logs / "sampler.json").write_text(json.dumps(SUMMARY))
            rec = ef.theseus_record(logs)
            self.assertEqual((rec["wall_s"], rec["wall_from"]), (40.0, "sampler"))
            path = ef.write(logs, rec)
            self.assertEqual(json.loads(path.read_text()), rec)
            meta = ef.metadata({"exit_code": 0}, rec)
            self.assertEqual((meta["exit_code"], meta["efficiency"]), (0, rec))


# Claude Code's session log: one line per content block, each repeating the
# message's usage (the last line's is the final one), and a subagent's file.
MSG_A = {"id": "msg_a", "model": SONNET, "role": "assistant"}
SESSION = [
    {"type": "user", "message": {"role": "user", "content": "Fix the repository."}},
    {"type": "assistant", "uuid": "u1", "message": dict(MSG_A, content=[{"type": "text", "text": "Looking."}],
                                                       usage=usage(10, 0, 4000, 1))},
    {"type": "assistant", "uuid": "u2", "message": dict(MSG_A, content=[
        {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {}}], usage=usage(10, 0, 4000, 30))},
    {"type": "assistant", "uuid": "u3", "message": dict(MSG_A, content=[
        {"type": "tool_use", "id": "toolu_2", "name": "Read", "input": {}}], usage=usage(10, 0, 4000, 55))},
    {"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]}},
    {"type": "assistant", "uuid": "u4", "message": {"id": "msg_b", "model": SONNET, "content": [
        {"type": "text", "text": "Done."}], "usage": usage(5, 4000, 200, 12)}},
]
RESULT = {"type": "result", "subtype": "success", "duration_ms": 41000, "num_turns": 3,
          "total_cost_usd": 0.0391,
          "usage": usage(15, 4000, 4200, 67),
          "modelUsage": {
              SONNET: {"inputTokens": 15, "outputTokens": 67, "cacheReadInputTokens": 4000,
                       "cacheCreationInputTokens": 4200, "costUSD": 0.0381},
              "claude-haiku-4-5": {"inputTokens": 600, "outputTokens": 20, "cacheReadInputTokens": 0,
                                   "cacheCreationInputTokens": 0, "costUSD": 0.001}}}
STREAM = "\n".join([json.dumps({"type": "system", "subtype": "init"}), "not json",
                    json.dumps({"type": "assistant"}), json.dumps(RESULT)]) + "\n"


class ClaudeCode(unittest.TestCase):
    def test_a_message_repeated_on_each_blocks_line_is_one_call(self):
        msgs = ef.session_messages(SESSION)
        self.assertEqual(sorted(msgs), ["msg_a", "msg_b"])
        self.assertEqual(msgs["msg_a"]["usage"]["output_tokens"], 55, "the last line's usage")
        self.assertEqual(msgs["msg_a"]["tools"], {"toolu_1", "toolu_2"})

    def test_the_result_event_is_the_bill_by_model(self):
        s = ef.claude_code_spend(STREAM, SESSION)
        self.assertEqual(s["spend_from"], "result_event")
        self.assertEqual(s["tokens"], {"input": 615, "cache_read": 4000, "cache_write": 4200,
                                       "output": 87})
        self.assertEqual(s["by_model"][SONNET]["calls"], 2)
        self.assertEqual(s["by_model"]["claude-haiku-4-5"],
                         {"input": 600, "cache_read": 0, "cache_write": 0, "output": 20,
                          "cost_usd": 0.001, "calls": 0})
        self.assertEqual((s["cost_usd"], s["model_calls"], s["tool_calls"]), (0.0391, 2, 2))

    def test_without_a_result_the_session_log_counts_each_message_once(self):
        """A timeout: no result event. The log's message A, on three lines,
        counts once with its last usage."""
        traj = {"final_metrics": {"total_cost_usd": 0.05}}
        s = ef.claude_code_spend("", SESSION, traj)
        self.assertEqual(s["spend_from"], "session_log")
        self.assertEqual(s["tokens"], {"input": 15, "cache_read": 4000, "cache_write": 4200,
                                       "output": 67})
        self.assertEqual((s["cost_usd"], s["model_calls"], s["tool_calls"]), (0.05, 2, 2))

    def test_a_trajectory_alone_keeps_the_cache_write_apart(self):
        """An old job's trajectory: prompt tokens hold input, read and write;
        the write is in each step's extra."""
        traj = {"agent": {"model_name": SONNET}, "final_metrics": {"total_cost_usd": 0.03},
                "steps": [
                    {"source": "user"},
                    {"source": "agent", "tool_calls": [{"tool_call_id": "t1"}],
                     "metrics": {"prompt_tokens": 1600, "cached_tokens": 1000, "completion_tokens": 40,
                                 "cost_usd": 0.02, "extra": {"cache_creation_input_tokens": 500}}},
                    {"source": "agent", "metrics": {"prompt_tokens": 1550, "cached_tokens": 1500,
                                                    "completion_tokens": 20, "cost_usd": 0.01}},
                ]}
        s = ef.trajectory_spend(traj)
        self.assertEqual(s["tokens"], {"input": 150, "cache_read": 2500, "cache_write": 500,
                                       "output": 60})
        self.assertEqual((s["spend_from"], s["cost_usd"], s["model_calls"], s["tool_calls"]),
                         ("trajectory", 0.03, 2, 1))
        self.assertEqual(s["by_model"][SONNET]["cost_usd"], 0.03)

    def test_the_record_reads_a_trials_agent_directory(self):
        with tempfile.TemporaryDirectory() as t:
            logs = Path(t)
            (logs / "claude-code.txt").write_text(STREAM)
            proj = logs / "sessions" / "projects" / "-app"
            (proj / "ses" / "subagents").mkdir(parents=True)
            (proj / "ses.jsonl").write_text("\n".join(json.dumps(e) for e in SESSION[:4]) + "\n")
            (proj / "ses" / "subagents" / "agent-1.jsonl").write_text(
                "\n".join(json.dumps(e) for e in SESSION[4:]) + "\n")
            rec = ef.claude_code_record(logs)
            self.assertEqual((rec["arm"], rec["model_calls"], rec["tool_calls"], rec["cost_usd"]),
                             ("claude-code", 2, 2, 0.0391))
            self.assertEqual((rec["wall_s"], rec["wall_from"]), (41.0, "agent"))


SUMMARY = {
    "status": "ok", "reason": None, "interval_ms": 250, "samples": 160, "wall_s": 40.0,
    "classes": {
        "harness": {"cpu_s": 1.2, "peak_rss_kb": 52000, "peak_hwm_kb": 48000, "processes": 2},
        "wrapper": {"cpu_s": 0.1, "peak_rss_kb": 3000, "peak_hwm_kb": 2900, "processes": 4},
        "work": {"cpu_s": 30.0, "peak_rss_kb": 900000, "peak_hwm_kb": 700000, "processes": 41},
        "outside": {"cpu_s": 0.2, "peak_rss_kb": 0, "peak_hwm_kb": 0, "processes": 3},
    },
    "cgroup": {"path": "/sys/fs/cgroup", "cpu_s": 33.0, "memory_peak_kb": 2400000},
    "sampler": {"cpu_s": 0.08, "core_share": 0.002},
}


class Machine(unittest.TestCase):
    def test_the_work_is_the_containers_cpu_less_the_rest(self):
        m = ef.machine(SUMMARY)
        self.assertEqual(m["harness"], {"cpu_s": 1.2, "peak_rss_kb": 52000, "peak_hwm_kb": 48000,
                                        "processes": 2})
        self.assertEqual(m["wrappers"]["cpu_s"], 0.1)
        # 33.0 - 1.2 - 0.1 - 0.2: outside's 0.2 holds the sampler's own 0.08, counted once
        self.assertEqual((m["work"]["cpu_s"], m["work"]["cpu_s_sampled"], m["work"]["cpu_from"]),
                         (31.5, 30.0, "cgroup"))
        self.assertEqual(m["container"], {"cpu_s": 33.0, "memory_peak_kb": 2400000})
        self.assertEqual(m["sampler"]["status"], "ok")

    def test_the_sampler_is_counted_once(self):
        """The sampler's pid is in the outside class, so its CPU is not taken
        off the cgroup's total a second time: the work is the total less the
        harness, the wrappers and outside, whatever `sampler.cpu_s` says."""
        for own in (0.0, 0.08, 5.0):
            m = ef.machine(dict(SUMMARY, sampler={"cpu_s": own, "core_share": 0.0}))
            self.assertEqual(m["work"]["cpu_s"], 31.5)

    def test_without_a_cgroup_the_work_is_the_samples(self):
        m = ef.machine(dict(SUMMARY, cgroup=None))
        self.assertEqual((m["work"]["cpu_s"], m["work"]["cpu_from"]), (30.0, "samples"))
        self.assertIsNone(m["container"])

    def test_a_sampler_that_did_not_run_says_why(self):
        m = ef.machine({"status": "unavailable", "reason": "no python3 on PATH"})
        self.assertEqual((m["harness"], m["work"]), (None, None))
        self.assertEqual((m["sampler"]["status"], m["sampler"]["reason"]),
                         ("unavailable", "no python3 on PATH"))
        stale = ef.machine(dict(SUMMARY, status="running"))
        self.assertEqual(stale["harness"]["cpu_s"], 1.2)
        self.assertIn("never wrote its final summary", stale["sampler"]["reason"])

    def test_each_arm_names_its_harness(self):
        self.assertEqual(ef.ARMS["theseus"]["names"], ("theseus", "theseusd"))
        self.assertIn("job-wrapper", ef.ARMS["theseus"]["wrapper_args"])
        self.assertEqual(ef.ARMS["claude-code"]["names"], ("claude",))


if __name__ == "__main__":
    unittest.main()

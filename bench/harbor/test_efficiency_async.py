"""The async bench's efficiency records (theseus-z5ty): Theseus's from its
daemon's ledger, and Claude Code's results counted, over fixture files.

    python3 -m unittest discover -s bench/harbor
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import efficiency as ef  # noqa: E402

# A sampler's summary of the real shape, without a cgroup.
SUMMARY = {
    "status": "ok", "reason": None, "interval_ms": 250, "samples": 400, "wall_s": 100.0,
    "classes": {
        "harness": {"cpu_s": 2.5, "peak_rss_kb": 40960, "peak_hwm_kb": 36000, "processes": 3},
        "wrapper": {"cpu_s": 0.1, "peak_rss_kb": 2000, "peak_hwm_kb": 2000, "processes": 2},
        "work": {"cpu_s": 12.0, "peak_rss_kb": 90000, "peak_hwm_kb": 60000, "processes": 6},
        "outside": {"cpu_s": 0.4, "peak_rss_kb": 0, "peak_hwm_kb": 0, "processes": 9},
    },
    "cgroup": None,
    "sampler": {"cpu_s": 0.3, "core_share": 0.003},
}


def call(session: str, model: str, cost: float | None, i: int, o: int, read: int = 0, write: int = 0) -> dict:
    """A `provider.call` row as `theseus ledger -k provider.call` prints it."""
    return {"kind": "provider.call", "session_id": session,
            "data": {"model": model, "cost_usd": cost, "provider": "anthropic",
                     "usage": {"input_tokens": i, "output_tokens": o, "cache_read_input_tokens": read,
                               "cache_creation_input_tokens": write}}}


ROWS = [
    # The conversation's first ask: two calls (what theseus-turn.json counts).
    call("ses_talk", "claude-sonnet-5-5", 0.01, 100, 40, 1000, 500),
    call("ses_talk", "claude-sonnet-5-5", 0.0068, 20, 30, 1500, 0),
    # The injected turn, the job's continuation, and a task's session.
    call("ses_talk", "claude-sonnet-5-5", 0.009, 10, 60, 1600, 100),
    call("ses_talk", "claude-sonnet-5-5", 0.0075, 5, 50, 1700, 0),
    call("ses_task", "claude-haiku-4-5", 0.009, 300, 200),
]


class LedgerRecord(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, name: str, value) -> None:
        (self.logs / name).write_text(json.dumps(value))

    def test_the_spend_is_every_row_by_model(self):
        s = ef.ledger_spend(ROWS + [{"kind": "turn.ended", "data": {"cost_usd": 9.0}}])
        self.assertEqual((s["spend_from"], s["model_calls"], s["cost_usd"]), ("ledger", 5, 0.0423))
        self.assertEqual(s["by_model"]["claude-sonnet-5-5"],
                         {"input": 135, "cache_read": 5800, "cache_write": 600, "output": 180,
                          "cost_usd": 0.0333, "calls": 4})
        self.assertEqual(s["by_model"]["claude-haiku-4-5"]["calls"], 1)
        self.assertEqual(s["tokens"], {"input": 435, "cache_read": 5800, "cache_write": 600, "output": 380})

    def test_a_row_with_no_price_leaves_the_dollars_unknown(self):
        s = ef.ledger_spend(ROWS + [call("ses_talk", "claude-invented", None, 1, 1)])
        self.assertIsNone(s["cost_usd"])
        self.assertEqual(s["model_calls"], 6)

    def test_the_record_is_the_ledgers_and_the_samplers(self):
        self.write("theseus-calls.json", {"rows": ROWS, "total": len(ROWS)})
        self.write("theseus-history.json", {"nodes": [{"kind": "tool_call"}, {"kind": "tool_call"},
                                                      {"kind": "assistant_message"}]})
        # The first ask's turn alone: what the inherited record would read.
        self.write("theseus-turn.json", {"cost_usd": 0.0168, "loops": 2, "usage": {"input_tokens": 120}})
        self.write(ef.SAMPLER_SUMMARY, SUMMARY)
        rec = ef.theseus_ledger_record(self.logs, wall_s=130.0)
        self.assertEqual((rec["schema"], rec["arm"], rec["spend_from"]), (ef.SCHEMA, "theseus", "ledger"))
        self.assertEqual((rec["model_calls"], rec["cost_usd"], rec["tool_calls"]), (len(ROWS), 0.0423, 2))
        self.assertEqual((rec["sampler"]["status"], rec["harness"]["cpu_s"], rec["harness"]["peak_rss_kb"]),
                         ("ok", 2.5, 40960))
        self.assertEqual(rec["wrappers"]["cpu_s"], 0.1)
        # The sampler's window, not the trial's wall, when it ran.
        self.assertEqual((rec["wall_s"], rec["wall_from"]), (100.0, "sampler"))

    def test_without_a_sampler_the_wall_is_the_trials(self):
        self.write("theseus-calls.json", {"rows": ROWS, "total": len(ROWS)})
        rec = ef.theseus_ledger_record(self.logs, wall_s=130.0)
        self.assertEqual((rec["wall_s"], rec["wall_from"], rec["sampler"]["status"]), (130.0, "agent", "missing"))
        self.assertIsNone(rec["harness"])


def result(cost: float, usage: dict[str, dict]) -> str:
    return json.dumps({"type": "result", "subtype": "success", "total_cost_usd": cost, "num_turns": 1,
                       "modelUsage": usage}, separators=(",", ":"))


def usage(i: int, o: int, read: int, write: int, cost: float) -> dict:
    return {"inputTokens": i, "outputTokens": o, "cacheReadInputTokens": read,
            "cacheCreationInputTokens": write, "costUSD": cost}


# Two messages, two turns, two results: the second's numbers are the first's
# and its own turn's (the session's so far).
STREAM = "\n".join([
    json.dumps({"type": "system", "subtype": "init"}),
    result(0.02, {"claude-sonnet-5-5": usage(10, 100, 2000, 800, 0.02)}),
    result(0.035, {"claude-sonnet-5-5": usage(15, 160, 4500, 900, 0.035)}),
]) + "\n"


class ClaudeCodeResults(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)
        (self.logs / "claude-code.txt").write_text(STREAM)

    def tearDown(self):
        self.tmp.cleanup()

    def test_every_result_is_read_in_order(self):
        self.assertEqual([r["total_cost_usd"] for r in ef.result_events(STREAM)], [0.02, 0.035])

    def test_the_last_result_is_the_sessions_bill_and_the_results_are_counted(self):
        rec = ef.claude_code_async_record(self.logs)
        self.assertEqual((rec["result_events"], rec["result_reading"]), (2, "session"))
        self.assertEqual((rec["spend_from"], rec["cost_usd"]), ("result_event", 0.035))
        self.assertEqual(rec["tokens"], {"input": 15, "cache_read": 4500, "cache_write": 900, "output": 160})

    def test_read_per_turn_the_results_are_summed(self):
        rec = ef.claude_code_async_record(self.logs, per_turn=True)
        self.assertEqual((rec["result_events"], rec["result_reading"]), (2, "per_turn"))
        self.assertEqual(rec["cost_usd"], 0.055)
        self.assertEqual(rec["tokens"], {"input": 25, "cache_read": 6500, "cache_write": 1700, "output": 260})
        self.assertEqual(rec["by_model"]["claude-sonnet-5-5"]["cost_usd"], 0.055)

    def test_one_result_reads_the_same_either_way(self):
        (self.logs / "claude-code.txt").write_text(STREAM.splitlines()[1] + "\n")
        a, b = ef.claude_code_async_record(self.logs), ef.claude_code_async_record(self.logs, per_turn=True)
        self.assertEqual((a["cost_usd"], b["cost_usd"], a["result_events"]), (0.02, 0.02, 1))


if __name__ == "__main__":
    unittest.main()

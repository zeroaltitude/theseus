"""bench/view/proof.py (issue 44): the view's end-to-end proof and its cost table.

The pure parts are tested here: the stand-in's script (rounds, a marker that opens a turn once, a refusal for
size, a writer's call), the shared-prefix measure (cache marks taken out, a provider cache's order), the case
settings (the mode's assembly, the kit's own rules last), the suite's case list against the addendum's, and the
table's words. One test runs a case in both modes on the real binaries (target/debug) when they are built, and
skips otherwise. The suite reads TOML with `tomllib`, so on a Python older than 3.11 the whole file skips.
"""

from __future__ import annotations

import io
import contextlib
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEW = ROOT / "bench" / "view"
sys.path[:0] = [str(VIEW), str(ROOT / "bench" / "recall")]
NEW_ENOUGH = sys.version_info >= (3, 11)
if NEW_ENOUGH:
    import proof  # noqa: E402
    import scratch  # noqa: E402

TOOLS = [{"name": "fs_list", "input_schema": {"type": "object"}}]


def user(text: str) -> dict:
    return {"role": "user", "content": [{"type": "text", "text": text}]}


def result_msg() -> dict:
    return {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "x"}]}


def call_msg() -> dict:
    return {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "fs_list", "input": {}}]}


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class Measures(unittest.TestCase):
    def test_a_cache_mark_does_not_change_the_bytes(self):
        a = {"tools": TOOLS, "system": [{"type": "text", "text": "s"}], "messages": [user("hi")]}
        b = json.loads(json.dumps(a))
        b["system"][0]["cache_control"] = {"type": "ephemeral"}
        b["messages"][0]["content"][0]["cache_control"] = {"type": "ephemeral"}
        self.assertEqual(proof.request_bytes(a), proof.request_bytes(b))

    def test_the_bytes_are_in_a_caches_order(self):
        req = {"messages": [user("MSG")], "system": [{"type": "text", "text": "SYS"}], "tools": [{"name": "TOOL"}]}
        b = proof.request_bytes(req)
        self.assertLess(b.index(b"TOOL"), b.index(b"SYS"))
        self.assertLess(b.index(b"SYS"), b.index(b"MSG"))

    def test_the_shared_prefix(self):
        self.assertEqual(proof.shared_prefix(b"abcdef", b"abcdef"), 6)
        self.assertEqual(proof.shared_prefix(b"abcdef", b"abcxyz"), 3)
        self.assertEqual(proof.shared_prefix(b"abc", b"abcdef"), 3)
        self.assertEqual(proof.shared_prefix(b"x", b"y"), 0)
        self.assertEqual(proof.shared_prefix(b"", b"y"), 0)

    def test_a_turns_rounds_and_its_opening_text(self):
        req = {"messages": [user("old"), {"role": "assistant", "content": "a"}, user("Open it"), call_msg(),
                            result_msg(), call_msg(), result_msg()]}
        self.assertEqual(proof.rounds(req), 2)
        self.assertEqual(proof.turn_text(req), "Open it")
        self.assertEqual(proof.rounds({"messages": [user("Open it")]}), 0)
        self.assertEqual(proof.turn_text({"messages": [{"role": "user", "content": "plain"}]}), "plain")


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class StandIn(unittest.TestCase):
    def setUp(self):
        self.m = proof.Model()
        self.addCleanup(self.m.close)

    def kinds(self, events) -> list[str]:
        return [e["content_block"]["type"] for e in events if e["type"] == "content_block_start"]

    def test_steps_by_round_then_the_reply(self):
        ls = {"calls": [{"name": "fs_list", "input": {"path": "."}}]}
        self.m.script = {"List it": [ls, ls, {"text": "Listed."}]}
        first = {"tools": TOOLS, "messages": [user("List it now")]}
        self.assertEqual(self.kinds(self.m.answer(first)[1]), ["tool_use"])
        second = {"tools": TOOLS, "messages": [user("List it now"), call_msg(), result_msg()]}
        self.assertEqual(self.kinds(self.m.answer(second)[1]), ["tool_use"])
        third = {"tools": TOOLS, "messages": [*second["messages"], call_msg(), result_msg()]}
        status, events = self.m.answer(third)
        self.assertEqual((status, self.kinds(events)), (200, ["text"]))
        self.assertIn("Listed.", json.dumps(events))
        self.assertEqual([s["round"] for s in self.m.calls_of("List it")], [0, 1, 2])

    def test_a_marker_opens_a_turn_once(self):
        """A wake's or a task's turn that quotes the words which set it up must not set it up again."""
        self.m.script = {"Wake me": [{"calls": [{"name": "wake_at", "input": {"after": "2s"}}]}]}
        req = {"tools": TOOLS, "messages": [user("Wake me soon")]}
        self.assertEqual(self.kinds(self.m.answer(req)[1]), ["tool_use"])
        again = {"tools": TOOLS, "messages": [user("[A wake] you said: Wake me soon")]}
        self.assertEqual(self.kinds(self.m.answer(again)[1]), ["text"])

    def test_the_first_marker_in_the_scripts_order_wins(self):
        self.m.script = {"Survey the quay": [{"text": "task"}], "Please start": [{"text": "parent"}]}
        _, events = self.m.answer({"tools": TOOLS, "messages": [user("[Task] Survey the quay. Please start")]})
        self.assertIn('"task"', json.dumps(events))

    def test_a_refusal_for_size_then_an_answer(self):
        self.m.script = {"Refuse": [{"too_long": True}]}
        req = {"tools": TOOLS, "messages": [user("Refuse this")]}
        status, body = self.m.answer(req)
        self.assertEqual(status, 400)
        self.assertIn("prompt is too long", body["error"]["message"])
        self.assertEqual(self.m.answer(req)[0], 200)

    def test_a_writers_call_offers_no_tools_and_takes_no_step(self):
        self.m.script = {"List it": [{"calls": [{"name": "fs_list", "input": {}}]}]}
        status, events = self.m.answer({"messages": [user("List it")]})
        self.assertEqual(self.kinds(events), ["text"])
        self.assertIsNone(self.m.seen[-1]["marker"])

    def test_the_stand_in_answers_over_http(self):
        import urllib.request
        body = json.dumps({"tools": TOOLS, "messages": [user("hello")]}).encode()
        r = urllib.request.urlopen(urllib.request.Request(self.m.base + "/v1/messages", data=body), timeout=10)
        self.assertIn(b"message_stop", r.read())
        self.assertEqual(len(self.m.seen), 1)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class Settings(unittest.TestCase):
    def test_the_view_mode_sets_the_view_and_the_default_mode_nothing(self):
        p = Path("/tmp/projects-of-a-case")
        self.assertEqual(proof.settings("view", p)["context"]["assembly"], "view")
        self.assertNotIn("assembly", proof.settings("default", p).get("context") or {})
        self.assertEqual(proof.settings("default", p)["tools"]["projects_dir"], str(p))

    def test_the_kits_rules_win_over_a_cases_settings(self):
        extra = {"model": {"api_base": "https://example.invalid"}, "kernel": {"spend_limit_usd": 999.0},
                 "discord": {"enabled": True}}
        cfg = scratch.config(proof.settings("view", Path("/tmp/p"), extra), "http://127.0.0.1:5")
        self.assertEqual(cfg["model"]["api_base"], "http://127.0.0.1:5")
        self.assertEqual(cfg["kernel"]["spend_limit_usd"], scratch.STAND_IN_LIMIT)
        self.assertFalse(cfg["discord"]["enabled"])
        self.assertEqual(cfg["context"]["assembly"], "view")

    def test_the_retired_keys_are_the_cuts_list(self):
        """X-DECISIONS and the map's §6.8: every key the cut retires is in the settings file the case writes."""
        want = {"context.kept_mb", "context.ledger", "memory.summary_profile", "cache.keep_warm_hours",
                "cache.keep_warm_every_minutes", "cache.threads_keep_warm_hours", "cache.keep_warm_min_tokens",
                "cache.stub_min_tokens", "cache.stub_after_turns", "profiles.bench.keep_warm_hours",
                "threads.resume_keep_turns", "threads.working_tokens"}

        def keys(d, pre=""):
            for k, v in d.items():
                yield from keys(v, f"{pre}{k}.") if isinstance(v, dict) else [f"{pre}{k}"]
        self.assertEqual(set(keys(proof.RETIRED)), want)
        self.assertEqual(proof.CASES["retired_keys"][1], proof.RETIRED)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class Words(unittest.TestCase):
    def test_every_case_of_the_addendum_is_named(self):
        self.assertEqual(set(proof.CASES), {
            "message_and_follow_up", "steer", "tool_loop", "continuation", "restart_mid_turn", "wake", "report",
            "task", "check", "fork", "over_budget", "provider_refusal", "seal_between_calls", "new_mark",
            "shared_place", "retired_keys"})

    def test_the_verdict_lines(self):
        self.assertEqual(proof.verdict_line({"case": "wake", "ok": True, "why": ""}), "case wake: ok")
        self.assertEqual(proof.verdict_line({"case": "x", "ok": False, "why": "no answer"}), "case x: FAILED no answer")

    def test_a_cell_says_ok_a_hand_back_or_failed_with_the_strategies(self):
        ok = {"ok": True, "fallback": {}, "strategies": {"view": 2}}
        back = {"ok": True, "fallback": {"continuation": 1}, "strategies": {"view": 1, "transcript": 1}}
        self.assertEqual(proof.cell(ok), "ok [view 2]")
        self.assertEqual(proof.cell(back), "hand-back(continuation x1) [transcript 1, view 1]")
        self.assertEqual(proof.cell({"ok": False, "fallback": {}, "strategies": {}}), "FAILED")
        self.assertEqual(proof.cell({"ok": False, "not_driven": True}), "not driven")

    def test_the_table_and_the_cost_table(self):
        rs = [{"case": "wake", "mode": m, "ok": True, "fallback": {}, "strategies": {m: 1}} for m in proof.MODES]
        t = proof.table(rs, proof.MODES)
        self.assertIn("| wake | ok [view 1] | ok [default 1] |", t)
        rows = [{"turn": 1, "tokens": 100, "shared_tokens": 0}, {"turn": 2, "tokens": 120, "shared_tokens": 90}]
        runs = [{"mode": "view", "rows": rows, "total_tokens": 220, "shared_tokens": 90, "unshared_tokens": 130,
                 "strategies": {"view": 2}, "fallback": {}},
                {"mode": "default", "rows": rows, "total_tokens": 220, "shared_tokens": 110,
                 "unshared_tokens": 110, "strategies": {"transcript": 2}, "fallback": {}}]
        c = proof.cost_table(runs)
        self.assertIn("| 2 | 120 | 90 | 120 | 90 |", c)
        self.assertIn("ratio view/default: total 1.00, not shared 1.18", c)


def built_bins() -> Path | None:
    d = ROOT / "target" / "debug"
    return d if all((d / b).is_file() for b in ("theseus", "theseusd")) else None


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class RealDaemon(unittest.TestCase):
    """A message and its follow-up, in both modes, on the real binaries when they are built: both answered."""

    def test_a_message_and_its_follow_up_in_both_modes(self):
        bins = built_bins()
        if bins is None:
            self.skipTest("target/debug has no theseus and theseusd")
        tmp = Path(tempfile.mkdtemp(prefix="view-proof-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            code = proof.main(["--bin-dir", str(bins), "--out", str(tmp / "out"),
                               "--cases", "message_and_follow_up", "--cost-turns", "2"])
        self.assertEqual(code, 0, buf.getvalue())
        self.assertIn("[view] case message_and_follow_up: ok", buf.getvalue())
        self.assertIn("[default] case message_and_follow_up: ok", buf.getvalue())
        r = json.loads((tmp / "out" / "proof.json").read_text())
        self.assertEqual([len(c["rows"]) for c in r["cost"]], [2, 2])


if __name__ == "__main__":
    unittest.main()

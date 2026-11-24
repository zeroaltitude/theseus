"""bench/view/proof.py (issue 44): the view's end-to-end proof and its cost table.

The pure parts are tested here: the stand-in's script (rounds, a marker that opens a turn once, a refusal for
size, a writer's call), the shared-prefix measure (cache marks taken out, a provider cache's order), the case
settings (the kit's own rules last), the suite's case list against the addendum's, and the table's words: one
context, so one column. One test runs a case on the real binaries (target/debug) when they are built, and skips
otherwise. The suite reads TOML with `tomllib`, so on a Python older than 3.11 the whole file skips.
"""

from __future__ import annotations

import io
import contextlib
import json
import shutil
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEW = ROOT / "bench" / "view"
sys.path[:0] = [str(VIEW), str(ROOT / "bench" / "recall"), str(ROOT)]
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

    def test_a_turn_takes_the_marker_not_yet_opened_and_keeps_it_through_its_calls(self):
        """A task's brief quotes the parent's words, and a view's beside block can quote a task's brief: a turn's
        first call takes the marker that has not opened a turn, and its later calls keep the turn's own."""
        ls = {"calls": [{"name": "fs_list", "input": {}}]}
        self.m.script = {"Survey": [ls, {"text": "surveyed"}], "Please start": [ls, {"text": "started"}],
                         "Check it": [{"text": "checked"}]}
        self.m.answer({"tools": TOOLS, "messages": [user("Please start")]})
        self.assertEqual(self.m.seen[-1]["marker"], "Please start")
        task = [user("[Task] Survey, as asked: Please start")]
        self.m.answer({"tools": TOOLS, "messages": task})
        self.assertEqual(self.m.seen[-1]["marker"], "Survey")
        _, events = self.m.answer({"tools": TOOLS, "messages": [*task, call_msg(), result_msg()]})
        self.assertIn("surveyed", json.dumps(events))
        _, events = self.m.answer({"tools": TOOLS, "messages": [user("[card: Survey] Check it")]})
        self.assertIn("checked", json.dumps(events))

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
    def test_proof_runs_one_context(self):
        """The view is the one context: no setting names an assembly, the suite has no `--assembly`, and its
        tables have one column of results."""
        p = Path("/tmp/projects-of-a-case")
        self.assertNotIn("assembly", proof.settings(p).get("context") or {})
        self.assertEqual(proof.settings(p)["tools"]["projects_dir"], str(p))
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            proof.main(["--out", "/tmp/never-made", "--assembly", "view"])
        t = proof.table([{"case": "wake", "ok": True, "fallback": {}, "strategies": {"view": 1}}])
        self.assertEqual(t.splitlines()[:3], ["| case | result |", "|---|---|", "| wake | ok [view 1] |"])

    def test_the_kits_rules_win_over_a_cases_settings(self):
        extra = {"model": {"api_base": "https://example.invalid"}, "kernel": {"spend_limit_usd": 999.0},
                 "discord": {"enabled": True}}
        cfg = scratch.config(proof.settings(Path("/tmp/p"), extra), "http://127.0.0.1:5")
        self.assertEqual(cfg["model"]["api_base"], "http://127.0.0.1:5")
        self.assertEqual(cfg["kernel"]["spend_limit_usd"], scratch.STAND_IN_LIMIT)
        self.assertFalse(cfg["discord"]["enabled"])

    def test_health_is_read_for_values_never_for_field_names(self):
        health = {"keep_warm_hours": 3, "warnings": ["[cache] keep_warm_hours is retired"], "n": {"x": ["stub"]}}
        self.assertEqual(sorted(proof.string_values(health)), ["[cache] keep_warm_hours is retired", "stub"])

    def test_a_retired_key_is_named_by_its_dotted_or_its_own_unmistakable_name(self):
        self.assertTrue(proof.named("cache.keep_warm_hours", "[cache] keep_warm_hours is retired"))
        self.assertTrue(proof.named("context.ledger", "context.ledger is ignored"))
        self.assertFalse(proof.named("context.ledger", "the ledger holds 12 rows"))

    def test_the_retired_keys_are_the_cuts_list(self):
        """X-DECISIONS and the map's §6.8: every key the cut retires is in the settings file the case writes."""
        want = {"context.kept_mb", "context.ledger", "memory.summary_profile", "cache.keep_warm_hours",
                "cache.keep_warm_every_minutes", "cache.threads_keep_warm_hours", "cache.keep_warm_min_tokens",
                "cache.stub_min_tokens", "cache.stub_after_turns", "profiles.bench.keep_warm_hours",
                "threads.resume_keep_turns", "threads.working_tokens"}

        self.assertEqual(set(proof.leaf_keys(proof.RETIRED)), want)
        self.assertEqual(proof.CASES["retired_keys"][1], proof.RETIRED)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class Words(unittest.TestCase):
    def test_every_case_of_the_addendum_is_named(self):
        self.assertEqual(set(proof.CASES), {
            "message_and_follow_up", "steer", "tool_loop", "continuation", "restart_mid_turn", "wake", "report",
            "task", "check", "fork", "over_budget", "provider_refusal", "new_mark",
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
        t = proof.table([{"case": "wake", "ok": True, "fallback": {}, "strategies": {"view": 1}}])
        self.assertIn("| wake | ok [view 1] |", t)
        rows = [{"turn": 1, "tokens": 100, "shared_tokens": 0}, {"turn": 2, "tokens": 120, "shared_tokens": 90}]
        run = {"rows": rows, "total_tokens": 220, "shared_tokens": 90, "unshared_tokens": 130,
               "strategies": {"view": 2}, "fallback": {}}
        c = proof.cost_table(run)
        self.assertIn("| 2 | 120 | 90 |", c)
        self.assertIn("| total | 220 | 90 |", c)
        self.assertIn("compilations {'view': 2}, hand-backs 0", c)
        self.assertNotIn("ratio", c)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class LongCost(unittest.TestCase):
    def test_the_tool_heavy_conversation_has_three_results_of_six_thousand_characters_a_turn(self):
        lines, script = proof.tool_heavy(20)
        self.assertEqual(len(lines), 20)
        self.assertEqual(len(set(lines)), 20)
        calls = script[lines[4]][0]["calls"]
        self.assertEqual(len(calls), 3)
        self.assertTrue(all(f"head -c {proof.TOOL_RESULT_CHARS}" in c["input"]["argv"][2] for c in calls))

    def test_no_turns_text_is_held_in_another(self):
        """A marker held in another turn's text would take the wrong script (`Model`, first marker wins)."""
        for conv, n in ((proof.short_conversation, 120), (proof.tool_heavy, 20)):
            lines, _ = conv(n)
            for a in lines:
                self.assertEqual([b for b in lines if a in b], [a])

    def test_the_writers_answer_is_a_title_and_an_account(self):
        title, account = proof.WRITER.split("\n", 1)
        self.assertTrue(title and account)
        m = proof.Model()
        self.addCleanup(m.close)
        m.answer({"max_tokens": 1200, "messages": [user("fold these")]})
        self.assertEqual(proof.WRITER_KINDS.get(m.seen[-1]["max_tokens"]), "fold")

    def test_the_long_table(self):
        row = {"turn": 1, "tokens": 100, "shared_tokens": 60, "calls": 1, "off_calls": 2, "off_tokens": 50,
               "off_kinds": {"did": 1, "fold": 1}, "strategies": {"view": 1}}
        run = {"case": "long_120", "rows": [row], "total_tokens": 100, "shared_tokens": 60,
               "unshared_tokens": 40, "off_calls": 2, "off_tokens": 50, "off_kinds": {"did": 1, "fold": 1},
               "unanswered": 0, "strategies": {"view": 1}, "fallback": {}}
        t = proof.long_table(run)
        self.assertIn("| 1 | 100 | 40 | 1 | 2 (did1 fold1) 50t | view |", t)
        self.assertIn("first requests 100 tokens, not shared 40", t)
        self.assertIn("off-turn 2 calls 50 tokens", t)
        self.assertNotIn("ratio", t)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class AllRequests(unittest.TestCase):
    """A cost run's `all requests` counts every request the stand-in was sent exactly once: each turn's first,
    a tool turn's follow-up, the calls off the turn, the extra ones, and any outside a turn. The first request of
    each turn and its shared prefix stay a measure of their own, named for what they are."""

    def test_a_two_step_tool_turn_counts_both_of_its_requests_once(self):
        plain, tool = "Turn 1: note the buoy.", "Turn 2: list the files."
        script = {tool: [{"calls": [{"name": "fs_list", "input": {"path": "."}}]}, {"text": "Listed."}]}
        models = []

        class Fake:
            def __init__(self, bin_dir, out, model, cfg):
                self.model = model
                models.append(model)

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def writer(d, text):  # a call that offers no tools: the ladder's, or anything the daemon writes
            d.model.answer({"max_tokens": 200, "messages": [user(text)]})

        def opened(d, label, cwd):
            writer(d, "written before any turn")
            return "s"

        def submit(d, line, **kw):
            first = {"tools": TOOLS, "system": [{"type": "text", "text": "S"}], "messages": [user(line)]}
            d.model.answer(first)
            if line == tool:  # its second request carries the first's call and a long result: a different size
                big = {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1",
                                                   "content": "a long listing " * 200}]}
                d.model.answer(dict(first, messages=[user(line), call_msg(), big]))
            writer(d, f"say what was done in: {line}")
            return {"output": "ok"}

        started = [{"session_id": "s", "turn_id": f"t{i}"} for i in (1, 2)]
        out = Path(tempfile.mkdtemp(prefix="view-proof-all-"))
        self.addCleanup(shutil.rmtree, out, True)
        with unittest.mock.patch.multiple(proof, Daemon=Fake, open_session=opened, submit=submit,
                                          writer_quiet=lambda m, *a: None,
                                          rows=lambda d, kind: started if kind == "turn.started" else [],
                                          observe=lambda d: {"strategies": {"view": 2}, "fallback": {}}):
            r = proof.cost_run(out, out, 2, "two", lambda n: ([plain, tool], script))
        seen = models[0].seen
        own = [x["counted"] for x in seen if x["tools"] and tool in proof.turn_text(x["req"])]
        self.assertEqual(len(own), 2)
        self.assertNotEqual(own[0], own[1])
        self.assertEqual(proof.all_tokens(r), sum(x["counted"] for x in seen))
        self.assertEqual(r["all_calls"], len(seen))
        self.assertEqual((r["rows"][1]["follow_calls"], r["rows"][1]["follow_tokens"]), (1, own[1]))
        self.assertEqual((r["outside_calls"], r["extra_calls"]), (1, 0))
        # The first request of each turn is its own measure: the turn-2 follow-up is not in it.
        firsts = [x["counted"] for x in seen if x["tools"] and x["round"] == 0]
        self.assertEqual(r["total_tokens"], sum(firsts))
        t = proof.long_table(r)
        self.assertIn(f"first requests {sum(firsts)} tokens", t)
        self.assertIn(f"all requests {len(seen)} calls {sum(x['counted'] for x in seen)} tokens", t)
        self.assertNotIn("\nsent ", t)


def opening(line: str) -> dict:
    """A turn's tool-offering request with no tool result yet: the stand-in counts it as round 0."""
    return {"tools": TOOLS, "system": [{"type": "text", "text": "S"}], "messages": [user(line)]}


def later(line: str, size: int) -> dict:
    """A request of `line`'s turn that carries a tool result: the stand-in counts it as a later round. `size`
    makes each one's tokens its own."""
    res = {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "r " * size}]}
    return dict(opening(line), messages=[user(line), call_msg(), res])


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class Parts(unittest.TestCase):
    """A cost run's parts (first, follow-up, off-turn, extra, outside a turn) give each captured request exactly
    one class, so they add to the total, which is every request the stand-in was sent; in any order a turn's
    requests arrive."""

    def cost(self, lines, script, submit, before=(), after=()):
        """`cost_run` on a fake daemon. `submit(model, line)` sends a turn's requests; `before` are calls with no
        tools sent as the session opens, `after` ones sent as the run reads its rows back, after the last turn's
        span. The run, and every request the stand-in was sent."""
        models = []

        class Fake:
            def __init__(self, bin_dir, out, model, cfg):
                self.model = model
                models.append(model)

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def writer(d, text):
            d.model.answer({"max_tokens": 200, "messages": [user(text)]})

        def opened(d, label, cwd):
            for text in before:
                writer(d, text)
            return "s"

        def observe(d):
            for text in after:
                writer(d, text)
            return {"strategies": {"view": len(lines)}, "fallback": {}}

        def sub(d, line, **kw):
            submit(d.model, line)
            return {"output": "ok"}

        started = [{"session_id": "s", "turn_id": f"t{i}"} for i in range(1, len(lines) + 1)]
        out = Path(tempfile.mkdtemp(prefix="view-proof-parts-"))
        self.addCleanup(shutil.rmtree, out, True)
        with unittest.mock.patch.multiple(proof, Daemon=Fake, open_session=opened, submit=sub,
                                          writer_quiet=lambda m, *a: None, observe=observe,
                                          rows=lambda d, kind: started if kind == "turn.started" else []):
            r = proof.cost_run(out, out, len(lines), "parts", lambda n: (list(lines), script))
        return r, models[0].seen

    def add_up(self, r, seen):
        """The parts add to the total, in calls and in tokens, per turn and for the run; the total is every
        request the stand-in was sent."""
        for x in r["rows"]:
            self.assertEqual((x["tokens"] is not None) + x["follow_calls"] + x["off_calls"] + x["extra_calls"],
                             x["all_calls"])
            self.assertEqual((x["tokens"] or 0) + x["follow_tokens"] + x["off_tokens"] + x["extra_tokens"],
                             x["all_tokens"])
        firsts = sum(1 for x in r["rows"] if x["tokens"] is not None)
        self.assertEqual(firsts + r["follow_calls"] + r["off_calls"] + r["extra_calls"] + r["outside_calls"],
                         r["all_calls"])
        self.assertEqual(r["total_tokens"] + r["follow_tokens"] + r["off_tokens"] + r["extra_tokens"]
                         + r["outside_tokens"], r["all_tokens"])
        self.assertEqual((r["all_calls"], r["all_tokens"]), (len(seen), sum(x["counted"] for x in seen)))

    def test_a_turn_whose_round_zero_comes_last_gives_three_requests_three_classes(self):
        """Rounds 1, 2, then 0, in a two-step turn: the round-0 request is the first, the earliest other is its
        one follow-up, and the one past the script is extra. None is in two classes."""
        line = "Turn 1: list the files."
        script = {line: [{"calls": [{"name": "fs_list", "input": {"path": "."}}]}, {"text": "Listed."}]}

        def submit(m, ln):
            for req in (later(ln, 100), later(ln, 300), opening(ln)):
                m.answer(req)

        r, seen = self.cost([line], script, submit)
        self.assertEqual([x["round"] for x in seen], [1, 2, 0])
        self.assertEqual(len({x["counted"] for x in seen}), 3)
        row = r["rows"][0]
        self.assertEqual(row["tokens"], seen[2]["counted"])
        self.assertEqual((row["follow_calls"], row["follow_tokens"]), (1, seen[0]["counted"]))
        self.assertEqual((row["extra_calls"], row["extra_tokens"]), (1, seen[1]["counted"]))
        self.add_up(r, seen)

    def test_several_follow_ups_in_one_turn(self):
        line = "Turn 1: read the logs."
        call = {"calls": [{"name": "fs_list", "input": {"path": "."}}]}
        script = {line: [call, call, call, {"text": "Read."}]}

        def submit(m, ln):
            for req in (opening(ln), later(ln, 100), later(ln, 200), later(ln, 300)):
                m.answer(req)

        r, seen = self.cost([line], script, submit)
        self.assertEqual([x["round"] for x in seen], [0, 1, 2, 3])
        row = r["rows"][0]
        self.assertEqual((row["follow_calls"], row["extra_calls"]), (3, 0))
        self.assertEqual(row["follow_tokens"], sum(x["counted"] for x in seen[1:]))
        self.add_up(r, seen)

    def test_a_request_after_the_last_turns_span_is_outside(self):
        line = "Turn 1: note the buoy."
        r, seen = self.cost([line], {}, lambda m, ln: m.answer(opening(ln)), after=("written after the turns",))
        self.assertEqual((r["outside_calls"], r["outside_tokens"]), (1, seen[-1]["counted"]))
        self.assertEqual(r["rows"][0]["all_calls"], 1)
        self.add_up(r, seen)

    def test_a_request_of_another_turn_inside_a_span_is_extra(self):
        line = "Turn 1: note the buoy."

        def submit(m, ln):
            m.answer(opening(ln))
            m.answer(opening("[a wake of its own]"))

        r, seen = self.cost([line], {}, submit)
        row = r["rows"][0]
        self.assertEqual(row["tokens"], seen[0]["counted"])
        self.assertEqual((row["extra_calls"], row["extra_tokens"], row["follow_calls"]), (1, seen[1]["counted"], 0))
        self.add_up(r, seen)

    def test_a_run_with_no_turn_has_every_request_outside(self):
        r, seen = self.cost([], {}, None, before=("written as it opens",), after=("written after",))
        self.assertEqual(r["rows"], [])
        self.assertEqual((r["outside_calls"], r["all_calls"]), (2, 2))
        self.add_up(r, seen)

    def test_parts_that_do_not_add_to_the_total_stop_the_run(self):
        """The run checks, in code, that its parts add to its total: a run whose parts count a request twice is
        refused, not printed."""
        good = {"rows": [{"tokens": 5, "follow_calls": 1, "follow_tokens": 3, "off_calls": 0, "off_tokens": 0,
                          "extra_calls": 0, "extra_tokens": 0, "all_calls": 2, "all_tokens": 8}],
                "total_tokens": 5, "follow_calls": 1, "follow_tokens": 3, "off_calls": 0, "off_tokens": 0,
                "extra_calls": 0, "extra_tokens": 0, "outside_calls": 1, "outside_tokens": 2,
                "all_calls": 3, "all_tokens": 10}
        proof.check_parts(good)
        twice = json.loads(json.dumps(good))
        twice["rows"][0].update(extra_calls=1, extra_tokens=5)
        twice.update(extra_calls=1, extra_tokens=5)
        with self.assertRaises(proof.PartsDisagree):
            proof.check_parts(twice)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class ExpectCut(unittest.TestCase):
    def res(self, case, ok=True, fallback=None, strategies=None, **kw):
        return {"case": case, "ok": ok, "why": "" if ok else "no answer",
                "fallback": fallback or {}, "strategies": strategies or {"view": 2}, **kw}

    def test_all_view_is_accepted(self):
        rs = [self.res("wake"), {"case": "new_mark", "ok": False, "not_driven": True}]
        costs = [{"case": "cost", "strategies": {"view": 34}, "fallback": {}}]
        self.assertEqual(proof.expect_cut(rs, costs), [])

    def test_every_offending_cell_is_named(self):
        rs = [self.res("wake"), self.res("check", strategies={"transcript": 2}),
              self.res("task", fallback={"continuation": 1}), self.res("fork", ok=False)]
        costs = [{"case": "long_120", "strategies": {"ring": 1, "view": 3}, "fallback": {}}]
        bad = proof.expect_cut(rs, costs)
        self.assertEqual(bad, ["check: compilations {'transcript': 2}",
                               "task: hand-back {'continuation': 1}",
                               "fork: FAILED no answer",
                               "cost long_120: compilations {'ring': 1, 'view': 3}"])

    def test_the_flag_exits_nonzero_on_an_offence_without_a_daemon(self):
        """`--expect-cut` with no daemon: an offending result read back is exit 2, and is written."""
        out = Path(tempfile.mkdtemp(prefix="view-proof-flag-")) / "out"
        self.addCleanup(shutil.rmtree, out.parent, True)
        buf = io.StringIO()
        bad = self.res("fork", ok=False)
        with contextlib.redirect_stdout(buf), unittest.mock.patch.object(proof, "run_case", lambda *a: bad):
            code = proof.main(["--out", str(out), "--cases", "fork", "--cost-turns", "0", "--expect-cut"])
        self.assertEqual(code, 2)
        self.assertIn("- fork: FAILED no answer", buf.getvalue())
        self.assertEqual(json.loads((out / "proof.json").read_text())["expect_cut"], ["fork: FAILED no answer"])


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class ExtraCalls(unittest.TestCase):
    def call(self, text, rnd=0, tools=True, system="S" * 300):
        req = {"system": [{"type": "text", "text": system}], "messages": [user(text)],
               "tools": [{"name": "fs_list"}, {"name": "wake_at"}] if tools else []}
        return {"req": req, "tools": tools, "round": rnd, "counted": 50, "max_tokens": 900}

    def test_a_call_not_of_the_turn_and_a_call_past_its_script_are_extra(self):
        mine = [self.call("Turn 20: x"), self.call("[something else]"), self.call("Turn 20: x", 1),
                self.call("no tools", tools=False)]
        self.assertEqual([proof.turn_text(x["req"]) for x in proof.extra_calls(mine, "Turn 20: x", 1)],
                         ["[something else]", "Turn 20: x"])
        self.assertEqual(proof.extra_calls(mine[:1] + mine[2:3], "Turn 20: x", 2), [])

    def test_a_probe_says_the_system_text_the_tools_and_the_opening(self):
        p = proof.call_probe(self.call("Turn 20: x"))
        self.assertEqual(p["system"], "S" * 200)
        self.assertEqual(p["tools"], ["fs_list", "wake_at"])
        self.assertEqual((p["opening"], p["max_tokens"], p["tokens"]), ("Turn 20: x", 900, 50))

    def test_the_cold_start_line_prices_the_whole_last_request(self):
        rows = [{"turn": 61, "tokens": 16000, "shared_tokens": 14000}]
        line = proof.cold_line({"rows": rows})
        self.assertIn("turn 61 after a lapse", line)
        self.assertIn("16000 tokens ($0.040 at the 5-minute write, $0.064 at the 1-hour write; 2000 not shared", line)


def built_bins() -> Path:
    from scripts.tests.built_bins import current_bins
    return current_bins(ROOT)


@unittest.skipUnless(NEW_ENOUGH, "the suite reads TOML with tomllib: Python 3.11 or newer")
class RealDaemon(unittest.TestCase):
    """A message and its follow-up, in the one context, on the real binaries when they are built: both answered."""

    def test_a_message_and_its_follow_up_in_both_modes(self):
        bins = built_bins()
        tmp = Path(tempfile.mkdtemp(prefix="view-proof-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            code = proof.main(["--bin-dir", str(bins), "--out", str(tmp / "out"),
                               "--cases", "message_and_follow_up", "--cost-turns", "2"])
        self.assertEqual(code, 0, buf.getvalue())
        self.assertIn("case message_and_follow_up: ok", buf.getvalue())
        r = json.loads((tmp / "out" / "proof.json").read_text())
        self.assertEqual(len(r["cost"]["rows"]), 2)


if __name__ == "__main__":
    unittest.main()

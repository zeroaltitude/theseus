"""bench/view's measuring kit (issue 44, §10 of the context design): the paired replay, the labelled cases and
their scorer, and the yardstick's windows, cap and dry run.

Most tests drive the kit against a stand-in `theseus` and `theseusd` (small Python scripts that answer from a
fixture) whose answers are full of marker strings, and assert that no marker reaches anything the kit prints or
writes: the kit writes ids, kinds and counts only. One test drives the real binaries (target/debug) when they are
built and have `view preview`, and skips otherwise. The kit reads TOML with `tomllib`, so on a Python older than
3.11 the whole file says it skipped.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEW = ROOT / "bench" / "view"
sys.path[:0] = [str(VIEW), str(ROOT / "bench" / "recall")]
NEW_ENOUGH = sys.version_info >= (3, 11)
if NEW_ENOUGH:
    import cases as vc  # noqa: E402
    import replay  # noqa: E402
    import scratch  # noqa: E402
    import yardstick as ys  # noqa: E402

MARKERS = ("ZEBRA-MARKER-7731", "QUOKKA-MARKER-0942", "OSPREY-MARKER-5518")
FAKE_ENV = "VIEW_KIT_FAKE"

FAKE_THESEUSD = """#!/usr/bin/env python3
import os, sys, time
a = sys.argv
state = a[a.index('--state-dir') + 1]
open(os.path.join(os.path.dirname(state), 'fake.pid'), 'w').write(str(os.getpid()))
while True:
    time.sleep(0.2)
"""

FAKE_THESEUS = """#!/usr/bin/env python3
import json, os, signal, sys
fx = json.load(open(os.environ['VIEW_KIT_FAKE']))
a = sys.argv[1:]
sock = a[a.index('--socket') + 1]
a = [x for i, x in enumerate(a) if x != '--socket' and (i == 0 or a[i - 1] != '--socket')]
if a and a[0] == '--json':
    a = a[1:]
cmd = a[0] if a else ''
if cmd == 'health':
    sys.exit(0)
if cmd == 'shutdown':
    pid = int(open(os.path.join(os.path.dirname(sock), 'fake.pid')).read())
    os.kill(pid, signal.SIGTERM)
    sys.exit(0)
if cmd == 'rpc' and a[1] == 'ledger.tail':
    p = json.loads(a[2])
    rows = [] if 'before' in p else [r for r in fx['ledger'] if r['kind'] == p.get('kind')]
    print(json.dumps({'rows': rows, 'total': len(rows)}))
    sys.exit(0)
if cmd == 'view' and a[1] == 'preview':
    if '--text' in a or '--message' in a:
        print('the kit never asks for text or sends a message', file=sys.stderr)
        sys.exit(3)
    if '--turns' in a:
        if fx.get('turns') is None:
            print("error: unexpected argument '--turns'", file=sys.stderr)
            sys.exit(2)
        print(json.dumps({'turns': fx['turns']}))
        sys.exit(0)
    print(json.dumps(fx['now']))
    sys.exit(0)
print('unknown: ' + ' '.join(a), file=sys.stderr)
sys.exit(1)
"""

PLACE = "local:/srv/" + MARKERS[0]


def turn_row(pos: int, ex: str, prev: str | None, prev_kind: str) -> dict:
    return {"position": pos, "at_unix_ms": pos, "kind": "context.turn", "session_id": "s1", "turn_id": ex,
            "data": {"exchange": ex, "place": PLACE, "prev": prev_kind, "prev_exchange": prev, "cold": False,
                     "overlap": False, "ask_text_never_written": MARKERS[1],
                     "sent": {"header": 1000, "tools": 3000, "words": 200 * pos, "total": 4000 + 200 * pos},
                     "judge": {"kind": MARKERS[2]}}}


def preview(as_of: str | None, held: list[str], fallback: str | None) -> dict:
    pieces = [{"handle": "x" + e, "kind": "exchanges", "first": e, "last": e, "form": "whole", "tokens": 50,
               "reasons": [MARKERS[1]], "label": "here", "text": f"{MARKERS[0]} said something private"}
              for e in held]
    p = {"place": PLACE, "session": "s1", "reader": "owner",
         "view": {"slots": [{"slot": "here", "pieces": pieces, "tokens": 50 * len(pieces)}]},
         "tally": {"slots": {"here": {"pieces": len(pieces), "tokens": 50 * len(pieces)},
                             "frame": {"pieces": 1, "tokens": 900}},
                   "dropped": {"over_budget": {"pieces": 2, "tokens": 300}},
                   "dropped_reasons": {MARKERS[2] + " a reason with words": {"pieces": 2, "tokens": 300}},
                   "tokens": 900 + 50 * len(pieces)},
         "fallback": fallback, "window": len(held), "folds": {"1": 2},
         "budget": {"provider_window": 200000, "reserve": 32000, "ceiling": 60000, "here_allowance": 150000},
         "earlier": {"sessions": 1, "turns": 3, "no_exchange": 0, "other_place": 0},
         "notes": [MARKERS[0]]}
    if as_of:
        p["as_of"] = as_of
    return p


def fixture(turns: bool) -> dict:
    ledger = [turn_row(1, "ex1", None, "none"), turn_row(2, "ex2", "ex1", "transcript"),
              turn_row(3, "ex3", "ex2", "transcript")]
    ledger.append({"position": 4, "at_unix_ms": 4, "kind": "context.turn", "session_id": "s2", "turn_id": "o1",
                   "data": {"exchange": "o1", "place": "local:/elsewhere", "prev": "none", "sent": {"total": 7}}})
    return {"ledger": ledger,
            "turns": [preview("ex1", [], None), preview("ex2", ["ex1"], None),
                      preview("ex3", [], "unledgered_history")] if turns else None,
            "now": preview(None, ["ex3"], None)}


def everything_written(out: Path, printed: str) -> str:
    """What the kit printed, and every file it left in its output directory (the daemon's log with them)."""
    return printed + "".join(p.read_text(errors="replace") for p in sorted(out.rglob("*")) if p.is_file())


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class FakeDaemon(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="view-kit-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.bins = self.tmp / "bin"
        self.bins.mkdir()
        for name, body in (("theseusd", FAKE_THESEUSD), ("theseus", FAKE_THESEUS)):
            (self.bins / name).write_text(body)
            (self.bins / name).chmod(0o755)
        self.store = self.tmp / "copy"
        (self.store / "store" / "wal").mkdir(parents=True)
        (self.store / "store" / "wal" / "000000001.seg").write_bytes(MARKERS[0].encode())

    def run_replay(self, turns: bool) -> tuple[Path, str, dict]:
        fx = self.tmp / f"fx-{turns}.json"
        fx.write_text(json.dumps(fixture(turns)))
        out = self.tmp / f"out-{turns}"
        buf = io.StringIO()
        with unittest.mock.patch.dict(os.environ, {FAKE_ENV: str(fx)}), contextlib.redirect_stdout(buf):
            replay.main(["--bin-dir", str(self.bins), "--store", str(self.store), "--place", PLACE,
                         "--turns", "3", "--out", str(out)])
        return out, buf.getvalue(), json.loads((out / "summary.json").read_text())

    def test_the_replay_pairs_each_exchange_and_writes_no_word_of_it(self):
        out, printed, s = self.run_replay(turns=True)
        rows = [json.loads(x) for x in (out / "rows.jsonl").read_text().splitlines()]
        self.assertEqual([r["exchange"] for r in rows], ["ex1", "ex2", "ex3"])
        self.assertEqual(s["exchanges"], 3)
        self.assertEqual(s["with_a_view_row"], 3)
        self.assertEqual(s["tokens"]["transcript"]["sum"]["total"], 3 * 4000 + 200 * 6)
        self.assertEqual(s["view_fallback"], {"takes_the_view": 2, "unledgered_history": 1})
        self.assertEqual(s["previous_exchange"]["transcript"], {"none": 1, "transcript": 2})
        self.assertEqual(s["previous_exchange"]["view"], {"no_previous": 1, "held": 1, "not_held": 1})
        self.assertEqual(rows[1]["prev_in_view_basis"], "ids")
        self.assertEqual(s["left_out_by_why"]["over_budget"], {"pieces": 6, "tokens": 900})
        self.assertEqual(s["left_out_by_reason"], {"other": 6})
        written = everything_written(out, printed)
        for m in MARKERS:
            self.assertNotIn(m, written)
        # The copy it was given is as it was: the daemon ran on a copy of the copy.
        self.assertEqual((self.store / "store" / "wal" / "000000001.seg").read_bytes(), MARKERS[0].encode())
        self.assertFalse(scratch.drive.processes_naming(out / "daemon"))

    def test_without_the_preview_s_turns_form_no_exchange_gets_a_view_row(self):
        out, printed, s = self.run_replay(turns=False)
        self.assertEqual(s["with_a_view_row"], 0)
        self.assertEqual(s["notes"], [replay.NO_TURNS])
        self.assertEqual(s["next_turn_view"]["window"], 1)
        for m in MARKERS:
            self.assertNotIn(m, everything_written(out, printed))

    def test_a_live_state_directory_or_a_directory_that_is_no_store_is_refused(self):
        with self.assertRaises(scratch.Refused):
            scratch.check_copy(self.store, {"THESEUS_STATE_DIR": str(self.store)})
        with self.assertRaises(scratch.Refused):
            scratch.check_copy(self.store / "store", {"THESEUS_STATE_DIR": str(self.store)})
        with self.assertRaises(scratch.Refused):
            scratch.check_copy(self.tmp / "bin", {})
        self.assertEqual(scratch.check_copy(self.store, {}), self.store.resolve())


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class Pieces(unittest.TestCase):
    def test_the_scratch_config_spends_nothing_and_calls_no_outside_model(self):
        t = scratch.config({"model": {"api_base": "https://api.example.invalid"}, "providers": {"a": {}}})
        self.assertEqual(t["model"]["api_base"], scratch.DEAD_API)
        self.assertEqual(t["providers"]["a"]["api_base"], scratch.DEAD_API)
        self.assertEqual(t["kernel"]["spend_limit_usd"], scratch.CENT)
        self.assertEqual(t["kernel"]["daily_spend_ceiling_mode"], "stop")
        self.assertFalse(t["index"]["enabled"] or t["web"]["enabled"] or t["discord"]["enabled"])
        with self.assertRaises(ValueError):
            scratch.config({}, api_base="https://api.example.invalid")

    def test_the_synthetic_set_is_valid_and_covers_every_class(self):
        s = vc.load(VIEW / "cases" / "synthetic.v1.json")
        self.assertTrue(20 <= len(s["cases"]) <= 30)
        self.assertEqual({c["class"] for c in s["cases"]}, set(vc.CLASSES))

    def test_a_case_naming_a_line_it_does_not_have_is_invalid(self):
        bad = {"format": vc.FORMAT, "cases": [{"id": "a", "class": "callback_here",
                                               "history": [{"id": "h1", "place": "p", "say": "x"}],
                                               "message": {"place": "p", "say": "y"}, "evidence": ["h9"]}]}
        with self.assertRaises(vc.Invalid):
            vc.check(bad)

    def test_the_scorer_says_held_missed_stale_alone_forbidden_and_absent(self):
        s = vc.load(VIEW / "cases" / "synthetic.v1.json")
        by = {c["id"]: c for c in s["cases"]}
        self.assertEqual(vc.verdict(by["here-01"], {"h1"})["verdict"], "held")
        self.assertEqual(vc.verdict(by["here-01"], {"h2"})["why"], ["evidence", "referent"])
        self.assertEqual(vc.verdict(by["fix-01"], {"h1", "h3"})["why"], ["evidence", "referent", "stale_alone"])
        self.assertEqual(vc.verdict(by["fix-01"], {"h1", "h2"})["verdict"], "held")
        self.assertEqual(vc.verdict(by["seal-01"], {"h1", "h2"})["why"], ["forbidden"])
        r = vc.score(s, {"strategy": "t", "held": {"here-01": ["h1"]}})
        self.assertEqual(r["by_class"]["callback_here"], {"held": 1, "absent": 3})
        self.assertEqual(r["by_class"]["corrected_fact"], {"absent": 4})

    def test_the_transcript_holds_its_own_place_and_the_view_its_pieces_ranges(self):
        s = vc.load(VIEW / "cases" / "synthetic.v1.json")
        c = {x["id"]: x for x in s["cases"]}["else-03"]
        self.assertEqual(vc.transcript_held(c), ["h3"])
        p = {"view": {"slots": [{"pieces": [{"kind": "exchanges", "first": "e1", "last": "e2"},
                                            {"kind": "facts", "first": "e3", "last": "e3"}]}]}}
        held = vc.view_held(p, {"h1": "e1", "h2": "e2", "h3": "e3"}, {"P2": ["e1", "e2"], "P1": ["e3"]})
        self.assertEqual(held, ["h1", "h2"])

    def test_every_line_is_judged_in_exactly_two_windows(self):
        for n in range(1, 80):
            seen = [0] * n
            for w in ys.windows(n):
                self.assertLessEqual(len(w), ys.WINDOW)
                for i in w:
                    seen[i] += 1
            self.assertEqual(set(seen), {2}, n)

    def test_the_yardstick_is_refused_without_a_cap(self):
        with self.assertRaises(ys.CapNeeded):
            ys.main(["--cases", str(VIEW / "cases" / "synthetic.v1.json"), "--dry-run"])

    def test_the_dry_run_calls_nothing_and_prints_the_calls_and_the_worst_case(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), unittest.mock.patch.object(ys, "FakeJudge") as fake, \
                unittest.mock.patch.object(ys, "ModelJudge") as model:
            ys.main(["--cases", str(VIEW / "cases" / "synthetic.v1.json"), "--dry-run", "--cap-usd", "1"])
        fake.assert_not_called()
        model.assert_not_called()
        v = json.loads(buf.getvalue())
        self.assertEqual(v["calls"], 2 * 24)  # every case's history fits one window: two windows each
        self.assertGreater(v["worst_case_usd"], 0)

    def test_the_fake_judge_s_comparison_counts_what_the_rules_miss_and_writes_no_word(self):
        s = vc.load(VIEW / "cases" / "synthetic.v1.json")
        probes = ys.probes_from_cases(s, None)
        r = ys.run(probes, ys.FakeJudge({c["id"]: set(c["evidence"]) for c in s["cases"]}), ys.DEFAULT_MODEL, 1.0)
        # The transcript's rule holds nothing of another place: every callback from elsewhere is the judge's alone.
        self.assertEqual(r["by_class"]["callback_elsewhere"]["judge_only"], 4)
        self.assertIsNone(r["stopped"])
        self.assertLessEqual(r["spent_usd"], 1.0)
        text = json.dumps(r)
        for c in s["cases"]:
            for h in c["history"]:
                self.assertNotIn(h["say"], text)

    def test_the_cap_stops_the_run_before_the_call_that_could_pass_it(self):
        s = vc.load(VIEW / "cases" / "synthetic.v1.json")
        probes = ys.probes_from_cases(s, None)
        one = ys.worst(ys.DEFAULT_MODEL, ys.SYSTEM, ys.prompt(probes[0]["message"], probes[0]["texts"]))
        judge = ys.FakeJudge({})
        r = ys.run(probes, judge, ys.DEFAULT_MODEL, one * 3.5)
        self.assertIsNotNone(r["stopped"])
        self.assertLessEqual(r["spent_usd"], one * 3.5)
        self.assertEqual(judge.calls, r["calls"])
        self.assertLess(r["calls"], ys.plan(probes, ys.DEFAULT_MODEL)["calls"])


def built_bins() -> Path | None:
    d = ROOT / "target" / "debug"
    return d if all((d / b).is_file() for b in ("theseus", "theseusd")) else None


def has_preview(d: Path) -> bool:
    return subprocess.run([str(d / "theseus"), "view", "preview", "--help"], capture_output=True).returncode == 0


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class RealDaemon(unittest.TestCase):
    """The kit on the real binaries, when they are built: a store built by script on the stand-in model, with
    marker strings in what was said, then replayed from a copy of it. Without `view preview` in the build, the
    transcript's side alone."""

    def test_a_built_store_replays_with_no_marker_in_anything_written(self):
        bins = built_bins()
        if bins is None:
            self.skipTest("target/debug has no theseus and theseusd")
        tmp = Path(tempfile.mkdtemp(prefix="view-kit-real-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        case = {"id": "m", "class": "callback_here",
                "history": [{"id": "h1", "place": "here", "say": f"The code word is {MARKERS[0]}."},
                            {"id": "h2", "place": "here", "say": f"And the second is {MARKERS[1]}."}],
                "message": {"place": "here", "say": "What was the code word?"},
                "evidence": ["h1"], "referent": "h1"}
        built = vc.build_case(case, bins, tmp / "build")
        self.assertIn("view" if has_preview(bins) else "view_not_built", built, built)
        self.assertEqual(built["transcript"], ["h1", "h2"])
        copy = tmp / "copy"
        shutil.copytree(tmp / "build" / "daemon" / "state", copy)
        out = tmp / "replay"
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            with scratch.Scratch(bins, tmp / "probe", store=copy) as d:
                place = scratch.ledger_rows(d, replay.TURN_KIND)[-1]["data"]["place"]
            replay.main(["--bin-dir", str(bins), "--store", str(copy), "--place", place, "--turns", "5",
                         "--out", str(out)])
        s = json.loads((out / "summary.json").read_text())
        self.assertEqual(s["exchanges"], 2)
        two = dict(case, id="m2", history=[*case["history"], {"id": "h3", "place": "there", "say": "x"}])
        self.assertNotIn("not_built", vc.build_case(two, bins, tmp / "build2"), "two places came out as one")
        self.assertGreater(s["tokens"]["transcript"]["sum"]["total"], 0)
        if not has_preview(bins):
            self.assertEqual(s["notes"], [replay.NO_PREVIEW])
        for m in MARKERS:
            self.assertNotIn(m, everything_written(out, buf.getvalue()))


if __name__ == "__main__":
    unittest.main()

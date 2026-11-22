"""bench/view's measuring kit (issue 44, §10 of the context design): the paired replay, the labelled cases and
their scorer, and the yardstick's windows, cap and dry run.

Most tests drive the kit against a stand-in `theseus` and `theseusd` (small Python scripts that answer from a
fixture beside them) whose answers, errors and stderr are full of marker strings, and assert that no marker
reaches anything the kit prints, raises or writes: the kit writes ids, kinds and counts only. They also inject
failures before and after the daemon starts, and give the caller sentinel credentials the daemon must not see.
One test drives the real binaries (target/debug) when they are built, and skips otherwise. The kit reads TOML
with `tomllib`, so on a Python older than 3.11 the whole file says it skipped.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import signal
import sys
import tempfile
import threading
import time
import unittest
import unittest.mock
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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

MARKERS = ("ZEBRAMARKER7731", "QUOKKA-MARKER-0942", "OSPREY-MARKER-5518")
SENTINEL_TOKEN = "sentinel-op-token-4417"
SETS = VIEW / "cases" / "synthetic.v1.json"


def tid(n: int) -> str:
    return f"turn_{n:032x}"


FAKE_THESEUSD = """#!/usr/bin/env python3
import json, os, sys, time
here = os.path.dirname(os.path.abspath(__file__))
fx = json.load(open(os.path.join(here, 'fx.json')))
a = sys.argv
state = a[a.index('--state-dir') + 1]
d = os.path.dirname(state)
json.dump(dict(os.environ), open(os.path.join(d, 'env.json'), 'w'))
open(os.path.join(d, 'fake.pid'), 'w').write(str(os.getpid()))
sys.stderr.write(fx.get('daemon_stderr', '') + '\\n')
sys.stderr.flush()
if fx.get('daemon') == 'exit':
    sys.exit(7)
while True:
    time.sleep(0.2)
"""

FAKE_THESEUS = """#!/usr/bin/env python3
import json, os, signal, sys
here = os.path.dirname(os.path.abspath(__file__))
fx = json.load(open(os.path.join(here, 'fx.json')))
a = sys.argv[1:]
sock = a[a.index('--socket') + 1]
a = [x for i, x in enumerate(a) if x != '--socket' and (i == 0 or a[i - 1] != '--socket')]
if a and a[0] == '--json':
    a = a[1:]
cmd = a[0] if a else ''
def fail(code=1):
    sys.stderr.write(fx.get('cli_stderr', 'error') + '\\n')
    sys.exit(code)
if cmd == 'health':
    sys.exit(1 if fx.get('health') == 'never' else 0)
if cmd == 'shutdown':
    pid = int(open(os.path.join(os.path.dirname(sock), 'fake.pid')).read())
    os.kill(pid, signal.SIGTERM)
    sys.exit(0)
if cmd == 'rpc' and a[1] == 'ledger.tail':
    if fx.get('ledger') == 'garbage':
        print(fx['cli_stderr'])
        sys.exit(0)
    p = json.loads(a[2])
    rows = [] if 'before' in p else [r for r in fx['ledger'] if r['kind'] == p.get('kind')]
    print(json.dumps({'rows': rows, 'total': len(rows)}))
    sys.exit(0)
if cmd == 'history':
    if fx.get('history') == 'garbage':
        print(fx['cli_stderr'])
        sys.exit(0)
    print(json.dumps({'nodes': [] if '--before' in a else fx.get('nodes', [])}))
    sys.exit(0)
if cmd == 'view' and a[1] == 'preview':
    if '--text' in a or '--message' in a:
        fail(3)
    if fx.get('preview') == 'fail':
        fail(1)
    if '--turns' in a:
        if fx.get('turns') is None:
            fail(2)
        print(json.dumps({'turns': fx['turns']}))
        sys.exit(0)
    print(json.dumps(fx['now']))
    sys.exit(0)
fail(1)
"""

PLACE = "local:/srv/" + MARKERS[0]


def turn_row(pos: int, ex: str, prev: str | None, prev_kind: str) -> dict:
    return {"position": pos, "at_unix_ms": pos, "kind": "context.turn", "session_id": "session_" + "a" * 32,
            "turn_id": ex,
            "data": {"exchange": ex, "place": PLACE, "prev": prev_kind, "prev_exchange": prev, "cold": False,
                     "overlap": False, "ask_text_never_written": MARKERS[1],
                     "sent": {"header": 1000, "tools": 3000, "words": 200 * pos, "total": 4000 + 200 * pos},
                     "judge": {"kind": MARKERS[2]}}}


def preview(as_of: str | None, ranges: list[tuple[str, str]], fallback: str | None) -> dict:
    pieces = [{"handle": "x", "kind": "exchanges", "first": f, "last": l, "form": "whole", "tokens": 50,
               "reasons": [MARKERS[0]], "label": "here", "text": f"{MARKERS[1]} said something private"}
              for f, l in ranges]
    p = {"place": PLACE, "session": "s1", "reader": "owner",
         "view": {"slots": [{"slot": "here", "pieces": pieces, "tokens": 50 * len(pieces)}]},
         "tally": {"slots": {"here": {"pieces": len(pieces), "tokens": 50 * len(pieces)},
                             "frame": {"pieces": 1, "tokens": 900}, MARKERS[0]: {"pieces": 1, "tokens": 5}},
                   "dropped": {"over_budget": {"pieces": 2, "tokens": 300}},
                   "dropped_reasons": {MARKERS[0]: {"pieces": 2, "tokens": 300},
                                       "window": {"pieces": 1, "tokens": 10}},
                   "tokens": 900 + 50 * len(pieces)},
         "fallback": fallback, "window": len(ranges), "folds": {"1": 2, MARKERS[0]: 1},
         "budget": {"provider_window": 200000, "reserve": 32000, "ceiling": 60000, "here_allowance": 150000},
         "earlier": {"sessions": 1, "turns": 3, "no_exchange": 0, "other_place": 0},
         "notes": [MARKERS[0]]}
    if as_of:
        p["as_of"] = as_of
    return p


def fixture(turns: bool) -> dict:
    e = [tid(i) for i in range(1, 5)]
    ledger = [turn_row(1, e[0], None, "none"), turn_row(2, e[1], e[0], "transcript"),
              turn_row(3, e[2], e[1], MARKERS[0]), turn_row(4, e[3], e[2], "place_tail")]
    ledger.append({"position": 5, "at_unix_ms": 5, "kind": "context.turn", "session_id": "s2", "turn_id": "o1",
                   "data": {"exchange": "o1", "place": "local:/elsewhere", "prev": "none", "sent": {"total": 7}}})
    return {"ledger": ledger,
            "turns": [preview(e[0], [], None), preview(e[1], [(e[0], e[0])], MARKERS[0]),
                      preview(e[2], [(e[0], e[0])], "unledgered_history"),
                      # e[3]'s previous exchange (e[2]) lies INSIDE a range e[1]..e[3]
                      preview(e[3], [(e[1], e[3])], None)] if turns else None,
            "now": preview(None, [(e[3], e[3])], None),
            "cli_stderr": f"error: {MARKERS[1]} {MARKERS[2]}",
            "daemon_stderr": f"panic: {MARKERS[0]} {MARKERS[1]}"}


def everything_written(out: Path, printed: str) -> str:
    """What the kit printed, and every file it left in its output directory."""
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
        # A link inside the store, to a file inside it: allowed, and copied as a file.
        (self.store / "inner-link").symlink_to(self.store / "store" / "wal" / "000000001.seg")
        self.live = self.tmp / "live"  # a disposable sentinel for the live state directory
        (self.live / "store" / "wal").mkdir(parents=True)
        self.env = {"THESEUS_STATE_DIR": str(self.live)}
        self.fx(fixture(True))
        self.stderr = io.StringIO()
        self.enterContext(contextlib.redirect_stderr(self.stderr))

    def fx(self, v: dict) -> None:
        (self.bins / "fx.json").write_text(json.dumps(v))

    def leaks(self, *texts: str) -> None:
        for t in (*texts, self.stderr.getvalue()):
            for m in MARKERS:
                self.assertNotIn(m, t)

    def run_replay(self) -> tuple[Path, str, dict]:
        out = self.tmp / "out"
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            replay.main(["--bin-dir", str(self.bins), "--store", str(self.store), "--place", PLACE,
                         "--turns", "4", "--out", str(out)])
        return out, buf.getvalue(), json.loads((out / "summary.json").read_text())

    def test_the_replay_pairs_each_exchange_and_writes_no_word_of_it(self):
        out, printed, s = self.run_replay()
        rows = [json.loads(x) for x in (out / "rows.jsonl").read_text().splitlines()]
        self.assertEqual([r["exchange"] for r in rows], [tid(i) for i in range(1, 5)])
        self.assertEqual({r["recorded"]["strategy"] for r in rows}, {"not_recorded"})
        self.assertEqual(s["recorded_strategy"], "not_recorded")
        self.assertEqual(s["with_a_view_row"], 4)
        self.assertEqual(s["tokens"]["recorded"]["sum"]["total"], 4 * 4000 + 200 * 10)
        # A single-token free word in an enum's field is `other`, field by field.
        self.assertEqual(s["view_fallback"], {"takes_the_view": 2, "other": 1, "unledgered_history": 1})
        self.assertEqual(s["previous_exchange"]["recorded"],
                         {"none": 1, "transcript": 1, "other": 1, "place_tail": 1})
        self.assertEqual(s["left_out_by_reason"], {"other": 8, "window": 4})
        self.assertIn("other", rows[0]["view"]["slots"])
        self.assertEqual(rows[0]["view"]["folds"], {"1": 2, "other": 1})
        # The previous exchange inside a range's interior is held.
        self.assertEqual((rows[3]["prev_in_view"], rows[3]["prev_in_view_basis"]), (True, "ids"))
        self.assertEqual(s["previous_exchange"]["view"], {"no_previous": 1, "held": 2, "not_held": 1})
        self.leaks(everything_written(out, printed))
        # The copy it was given is as it was; the daemon's copy and its process are gone.
        self.assertEqual((self.store / "store" / "wal" / "000000001.seg").read_bytes(), MARKERS[0].encode())
        self.assertFalse((out / "daemon" / "state").exists())
        self.assertFalse(scratch.drive.processes_naming(out / "daemon"))

    def test_a_range_the_order_cannot_resolve_is_unknown_not_absent(self):
        f = fixture(True)
        f["turns"][3] = preview(tid(4), [(tid(1), tid(99))], None)
        self.fx(f)
        out, printed, s = self.run_replay()
        rows = [json.loads(x) for x in (out / "rows.jsonl").read_text().splitlines()]
        self.assertEqual((rows[3]["prev_in_view"], rows[3]["prev_in_view_basis"]), (None, "unknown_range"))

    def test_without_the_preview_s_turns_form_no_exchange_gets_a_view_row(self):
        self.fx(fixture(False))
        out, printed, s = self.run_replay()
        self.assertEqual(s["with_a_view_row"], 0)
        self.assertEqual(s["notes"], [replay.NO_TURNS])
        self.assertEqual(s["next_turn_view"]["window"], 1)
        self.leaks(everything_written(out, printed))

    def test_a_live_store_a_link_out_of_the_copy_or_a_directory_that_is_no_store_is_refused(self):
        for bad in (self.live, self.live / "store", self.tmp / "bin"):
            with self.assertRaises(scratch.Refused):
                scratch.check_copy(bad, self.env)
        linked = self.tmp / "linked"
        linked.mkdir()
        (linked / "store").symlink_to(self.live / "store")  # a copy whose store IS the live store
        with self.assertRaises(scratch.Refused):
            scratch.check_copy(linked, self.env)
        deep = self.tmp / "deep"
        shutil.copytree(self.store, deep, symlinks=True)
        (deep / "store" / "wal" / "out").symlink_to(self.live)
        with self.assertRaises(scratch.Refused):
            scratch.check_copy(deep, self.env)
        self.assertEqual(scratch.check_copy(self.store, self.env), self.store.resolve())

    def test_an_output_inside_a_live_root_or_the_input_is_refused_before_anything_is_written(self):
        before = sorted(p for p in self.live.rglob("*"))
        sneaky = self.tmp / "sneaky"
        sneaky.symlink_to(self.live)
        for out in (self.live / "out", sneaky / "out", self.tmp, self.store / "out"):
            with self.assertRaises(scratch.Refused):
                scratch.check_out(out, self.store, self.env)
            with unittest.mock.patch.dict(os.environ, self.env), self.assertRaises(SystemExit):
                replay.main(["--bin-dir", str(self.bins), "--store", str(self.store), "--place", PLACE,
                             "--out", str(out)])
        self.assertEqual(sorted(p for p in self.live.rglob("*")), before)
        self.assertFalse((self.store / "out").exists())

    def test_the_copy_holds_no_link_and_lives_in_a_private_directory(self):
        with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env) as d:
            self.assertEqual(d.dir.stat().st_mode & 0o777, 0o700)
            self.assertFalse([p for p in d.state.rglob("*") if p.is_symlink()])
            self.assertTrue((d.state / "inner-link").is_file())

    def test_the_daemon_sees_no_credential_of_the_caller(self):
        token_file = self.tmp / "op-token"
        token_file.write_text(SENTINEL_TOKEN)
        caller = {"OP_SERVICE_ACCOUNT_TOKEN": SENTINEL_TOKEN, "THESEUS_OP_TOKEN_FILE": str(token_file),
                  "ANTHROPIC_API_KEY": SENTINEL_TOKEN, "SOME_PROVIDER_KEY": SENTINEL_TOKEN}
        with unittest.mock.patch.dict(os.environ, caller):
            with scratch.Scratch(self.bins, self.tmp / "o", store=self.store) as d:
                seen = json.loads((d.dir / "env.json").read_text())
                cli_env = d.env
        for env in (seen, cli_env):
            self.assertNotIn("OP_SERVICE_ACCOUNT_TOKEN", env)
            self.assertNotIn("THESEUS_OP_TOKEN_FILE", env)
            self.assertNotIn(SENTINEL_TOKEN, json.dumps(env))
            self.assertNotIn(str(token_file), json.dumps(env))
            self.assertLessEqual(set(env) - {"HOME", "ANTHROPIC_API_KEY"} - set(scratch.ENV_KEPT),
                                 {"XDG_STATE_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME",
                                  "XDG_RUNTIME_DIR"})
            self.assertTrue(env["HOME"].startswith(str((self.tmp / "o" / "daemon").resolve())))

    def test_a_failure_before_the_spawn_removes_the_copy(self):
        with unittest.mock.patch.object(scratch.drive, "toml_dumps", side_effect=OSError(MARKERS[0])), \
                self.assertRaises(OSError):
            scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env)
        self.assertFalse((self.tmp / "o" / "daemon").exists())

    def test_a_daemon_that_dies_at_start_leaves_no_copy_and_no_word(self):
        f = fixture(True)
        f["daemon"] = "exit"
        f["health"] = "never"  # a dead daemon answers no health
        self.fx(f)
        with self.assertRaises(scratch.Failed) as e:
            with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env):
                pass
        self.assertTrue(str(e.exception).startswith("E_DAEMON_EXIT"))
        self.assertFalse((self.tmp / "o" / "daemon" / "state").exists())
        self.leaks(str(e.exception), everything_written(self.tmp / "o", ""))

    def test_a_health_timeout_stops_the_daemon_and_removes_the_copy(self):
        f = fixture(True)
        f["health"] = "never"
        self.fx(f)
        with unittest.mock.patch.object(scratch, "SERVE_WAIT", 0.5), self.assertRaises(scratch.Failed) as e:
            with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env):
                pass
        self.assertEqual(str(e.exception), "E_DAEMON_TIMEOUT")
        self.assertFalse((self.tmp / "o" / "daemon" / "state").exists())
        self.assertFalse(scratch.drive.processes_naming(self.tmp / "o" / "daemon"))

    def test_sigterm_stops_the_daemon_and_removes_the_copy(self):
        before = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(SystemExit) as e:
            with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env):
                os.kill(os.getpid(), signal.SIGTERM)
                time.sleep(5)
        self.assertIn("E_SIGTERM", str(e.exception))
        self.assertFalse((self.tmp / "o" / "daemon" / "state").exists())
        self.assertFalse(scratch.drive.processes_naming(self.tmp / "o" / "daemon"))
        self.assertEqual(signal.getsignal(signal.SIGTERM), before)

    def test_a_copy_that_cannot_be_removed_is_said_loudly_with_its_path(self):
        with unittest.mock.patch.object(scratch.shutil, "rmtree"):
            with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env) as d:
                state = d.state
        self.assertIn("E_REMOVE", self.stderr.getvalue())
        self.assertIn(str(state), self.stderr.getvalue())
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(scratch.clean(self.tmp / "o"), 0)
        self.assertFalse((self.tmp / "o" / "daemon").exists())

    def test_errors_of_the_cli_the_ledger_and_the_history_are_fixed_codes(self):
        f = fixture(True)
        f["preview"] = "fail"
        f["ledger"] = "garbage"
        f["history"] = "garbage"
        self.fx(f)
        with scratch.Scratch(self.bins, self.tmp / "o", store=self.store, env=self.env) as d:
            for call in (lambda: d.json("view", "preview", PLACE), lambda: scratch.ledger_rows(d, "context.turn"),
                         lambda: scratch.history_nodes(d, "s1"),
                         lambda: ys.probes_from_store(d, PLACE, 5, 5)):
                with self.assertRaises(scratch.Failed) as e:
                    call()
                self.assertTrue(str(e.exception).startswith("E_"))
                self.leaks(str(e.exception), repr(e.exception.__cause__), repr(e.exception.__context__))
        self.leaks(everything_written(self.tmp / "o", ""))


class SSE(BaseHTTPRequestHandler):
    events: list[dict] = []

    def log_message(self, *a):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get("content-length", "0")))
        body = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in self.events).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class Pieces(unittest.TestCase):
    def setUp(self):
        self.cases = vc.load(SETS)
        self.prices = ys.Prices(ys.DEFAULT_MODEL, ys.API, None, None)

    def test_the_scratch_config_spends_nothing_and_calls_no_outside_model(self):
        t = scratch.config({"model": {"api_base": "https://api.example.invalid"}, "providers": {"a": {}}})
        self.assertEqual(t["model"]["api_base"], scratch.DEAD_API)
        self.assertEqual(t["providers"]["a"]["api_base"], scratch.DEAD_API)
        self.assertEqual(t["kernel"]["spend_limit_usd"], scratch.CENT)
        self.assertEqual(t["kernel"]["daily_spend_ceiling_mode"], "stop")
        self.assertFalse(t["index"]["enabled"] or t["web"]["enabled"] or t["discord"]["enabled"])
        with self.assertRaises(ValueError):
            scratch.config({}, api_base="https://api.example.invalid")

    def test_the_synthetic_set_is_valid_covers_every_class_and_names_no_real_city(self):
        self.assertTrue(20 <= len(self.cases["cases"]) <= 30)
        self.assertEqual({c["class"] for c in self.cases["cases"]}, set(vc.CLASSES))
        text = SETS.read_text()
        for city in ("Lisbon", "Porto"):
            self.assertNotIn(city, text)

    def test_a_case_naming_a_line_it_does_not_have_is_invalid(self):
        bad = {"format": vc.FORMAT, "cases": [{"id": "a", "class": "callback_here",
                                               "history": [{"id": "h1", "place": "p", "say": "x"}],
                                               "message": {"place": "p", "say": "y"}, "evidence": ["h9"]}]}
        with self.assertRaises(vc.Invalid):
            vc.check(bad)

    def test_the_scorer_says_held_missed_stale_alone_forbidden_absent_and_not_built(self):
        by = {c["id"]: c for c in self.cases["cases"]}
        self.assertEqual(vc.verdict(by["here-01"], {"h1"})["verdict"], "held")
        self.assertEqual(vc.verdict(by["here-01"], {"h2"})["why"], ["evidence", "referent"])
        self.assertEqual(vc.verdict(by["fix-01"], {"h1", "h3"})["why"], ["evidence", "referent", "stale_alone"])
        self.assertEqual(vc.verdict(by["fix-01"], {"h1", "h2"})["verdict"], "held")
        everything = {c["id"]: [h["id"] for h in c["history"]] for c in self.cases["cases"]}
        r = vc.score(self.cases, {"strategy": "t", "held": {"here-01": ["h1"], **{
            k: v for k, v in everything.items() if k.startswith(("seal", "task"))}}})
        self.assertEqual(r["by_class"]["callback_here"], {"held": 1, "absent": 3})
        self.assertEqual(r["by_class"]["corrected_fact"], {"absent": 4})
        # The eight cases the builder cannot make stay `not_built`, whatever a context claims for them.
        self.assertEqual(r["by_class"]["forbidden_disclosure"], {"not_built": 4})
        self.assertEqual(r["by_class"]["task_update"], {"not_built": 4})
        self.assertEqual(len(r["not_built"]), 8)

    def test_the_transcript_holds_its_own_place_and_the_view_its_pieces_ranges(self):
        c = {x["id"]: x for x in self.cases["cases"]}["else-03"]
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

    def test_the_yardstick_is_refused_without_a_cap_and_with_an_unpriced_model_or_endpoint(self):
        with self.assertRaises(ys.CapNeeded):
            ys.main(["--cases", str(SETS), "--dry-run"])
        for extra in (["--model", "claude-unknown-9"], ["--api-base", "https://proxy.example.invalid"],
                      ["--price-in", "1"]):
            with self.assertRaises(SystemExit) as e:
                ys.main(["--cases", str(SETS), "--dry-run", "--cap-usd", "1", *extra])
            self.assertNotEqual(e.exception.code, 0)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            ys.main(["--cases", str(SETS), "--dry-run", "--cap-usd", "1", "--model", "claude-unknown-9",
                     "--price-in", "1", "--price-out", "5"])
        self.assertTrue(json.loads(buf.getvalue())["fits_the_cap"])

    def test_the_dry_run_calls_nothing_and_compares_the_unrounded_worst_case(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), unittest.mock.patch.object(ys, "FakeJudge") as fake, \
                unittest.mock.patch.object(ys, "ModelJudge") as model:
            ys.main(["--cases", str(SETS), "--dry-run", "--cap-usd", "1"])
        fake.assert_not_called()
        model.assert_not_called()
        v = json.loads(buf.getvalue())
        self.assertEqual(v["calls"], 2 * 24)
        self.assertEqual(v["rules_basis"], ys.OWN_PLACE)
        worst = v["worst_case_usd"]
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            ys.main(["--cases", str(SETS), "--dry-run", "--cap-usd", repr(worst * (1 - 1e-12))])
        self.assertFalse(json.loads(buf.getvalue())["fits_the_cap"])

    def test_the_reservation_bounds_a_request_counted_as_one_token_a_byte(self):
        user = ys.prompt("é中" * 300, ["\U0001F600 x" * 200] * 24)
        need = (len(ys.SYSTEM.encode()) + len(user.encode())) * self.prices.pin + ys.MAX_OUT * self.prices.pout
        self.assertGreaterEqual(self.prices.reserve(ys.SYSTEM, user), need)

    def test_the_fake_judge_s_comparison_counts_what_the_rule_misses_and_keeps_unbuilt_cases_apart(self):
        probes, basis = ys.probes_from_cases(self.cases, None)
        r = ys.run(probes, ys.FakeJudge({c["id"]: set(c["evidence"]) for c in self.cases["cases"]}), self.prices,
                   1.0)
        self.assertEqual(r["by_class"]["callback_elsewhere"]["judge_only"], 4)
        self.assertIsNone(r["stopped"])
        self.assertLessEqual(r["spent_usd"], 1.0)
        self.assertEqual(set(r["not_built_fixture_only"]), {"task_update", "forbidden_disclosure"})
        self.assertNotIn("task_update", r["by_class"])
        self.assertEqual(sum(1 for t in r["turns"] if t.get("not_built")), 8)
        text = json.dumps(r)
        for c in self.cases["cases"]:
            for h in c["history"]:
                self.assertNotIn(h["say"], text)

    def test_the_cap_holds_when_usage_is_missing_or_the_stream_did_not_finish(self):
        probes, _ = ys.probes_from_cases(self.cases, None)
        one = self.prices.reserve(ys.SYSTEM, ys.prompt(probes[0]["message"], probes[0]["texts"]))
        calls = []

        def silent(p, idx, user):
            calls.append(1)
            return [0] * len(idx), {}, False

        cap = one * 3.5
        r = ys.run(probes, silent, self.prices, cap)
        self.assertTrue(r["stopped"].startswith("E_CAP"))
        self.assertLessEqual(r["spent_usd"], cap)
        self.assertGreater(r["spent_usd"], 0)  # nothing reported, the reservations stay spent
        self.assertEqual(r["reservations_kept"], len(calls))
        self.assertLess(len(calls), ys.plan(probes, self.prices, ys.DEFAULT_MODEL)["calls"])

    def test_the_cap_holds_on_a_judge_that_reports_one_token_a_byte(self):
        probes, _ = ys.probes_from_cases(self.cases, None)

        def heavy(p, idx, user):
            return [0] * len(idx), {"input_tokens": len(ys.SYSTEM.encode()) + len(user.encode()) + ys.OVERHEAD_TOKENS,
                                    "output_tokens": ys.MAX_OUT}, True

        cap = 0.004
        r = ys.run(probes, heavy, self.prices, cap)
        self.assertLessEqual(r["spent_usd"], cap)
        self.assertIsNotNone(r["stopped"])

    def test_unjudged_lines_are_in_no_cell_and_counted_as_coverage(self):
        probes, _ = ys.probes_from_cases(self.cases, None)
        r = ys.run(probes, lambda p, idx, user: (None, {"input_tokens": 1, "output_tokens": 1}, True),
                   self.prices, 1.0)
        self.assertEqual(r["total"], {"both": 0, "judge_only": 0, "rules_only": 0, "neither": 0})
        self.assertEqual(r["coverage"]["judged"], 0)
        self.assertGreater(r["coverage"]["lines"], 0)

    def test_a_stream_error_is_a_fixed_code_and_its_reservation_stays_spent(self):
        SSE.events = [{"type": "message_start", "message": {"usage": {"input_tokens": 5, "output_tokens": 1}}},
                      {"type": "error", "error": {"message": f"{MARKERS[0]} {MARKERS[1]}"}}]
        srv = ThreadingHTTPServer(("127.0.0.1", 0), SSE)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.server_close)
        self.addCleanup(srv.shutdown)
        base = f"http://127.0.0.1:{srv.server_address[1]}"
        prices = ys.Prices("x", base, 1.0, 5.0)
        probes, _ = ys.probes_from_cases(self.cases, None)
        r = ys.run(probes, ys.ModelJudge("x", base, "k"), prices, 1.0)
        self.assertEqual(r["stopped"], "E_JUDGE_STREAM")
        self.assertEqual(r["calls"], 1)
        self.assertGreaterEqual(r["spent_usd"], prices.reserve(ys.SYSTEM, ys.prompt(probes[0]["message"],
                                                                                     probes[0]["texts"])))
        for m in MARKERS:
            self.assertNotIn(m, json.dumps(r))
        SSE.events = [{"type": "message_start", "message": {"usage": {"input_tokens": 5, "output_tokens": 1}}},
                      {"type": "content_block_delta", "delta": {"type": "text_delta", "text": '{"scores": [2]}'}}]
        text, usage, done = ys.ask(base, "k", "x", ys.SYSTEM, "u")
        self.assertFalse(done)  # no message_stop: the caller keeps the reservation


def built_bins() -> Path | None:
    d = ROOT / "target" / "debug"
    return d if all((d / b).is_file() for b in ("theseus", "theseusd")) else None


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class RealDaemon(unittest.TestCase):
    """The kit on the real binaries, when they are built: a store built by script on the stand-in model, with
    marker strings in what was said, then replayed from a copy of it, in the kit's minimal environment.
    Without `view preview` in the build, the recorded side alone."""

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
        with unittest.mock.patch.object(scratch.Scratch, "stop", keep_state_stop):
            built = vc.build_case(case, bins, tmp / "build")
        # Whether the build has `view preview` is the daemon's answer, as the builder met it.
        preview = "view" in built
        self.assertTrue(preview or built.get("view_not_built") == "no_preview", built)
        self.assertEqual(built["transcript"], ["h1", "h2"])
        copy = tmp / "copy"
        shutil.copytree(tmp / "build" / "daemon" / "state", copy)
        shutil.rmtree(tmp / "build" / "daemon" / "state")
        out = tmp / "replay"
        buf, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(err):
            with scratch.Scratch(bins, tmp / "probe", store=copy) as d:
                place = scratch.ledger_rows(d, replay.TURN_KIND)[-1]["data"]["place"]
            replay.main(["--bin-dir", str(bins), "--store", str(copy), "--place", place, "--turns", "5",
                         "--out", str(out)])
        s = json.loads((out / "summary.json").read_text())
        self.assertEqual(s["exchanges"], 2)
        self.assertGreater(s["tokens"]["recorded"]["sum"]["total"], 0)
        if not preview:
            self.assertEqual(s["notes"], [replay.NO_PREVIEW])
        for m in MARKERS:
            self.assertNotIn(m, everything_written(out, buf.getvalue() + err.getvalue()))
        two = dict(case, id="m2", history=[*case["history"], {"id": "h3", "place": "there", "say": "x"}])
        self.assertNotIn("not_built", vc.build_case(two, bins, tmp / "build2"), "two places came out as one")


def keep_state_stop(self):
    """`Scratch.stop` without removing the state: the real test replays the store the builder made."""
    saved = self.state
    self.state = self.dir / "no-such-state"
    try:
        return SCRATCH_STOP(self)
    finally:
        self.state = saved


SCRATCH_STOP = scratch.Scratch.stop if NEW_ENOUGH else None


if __name__ == "__main__":
    unittest.main()

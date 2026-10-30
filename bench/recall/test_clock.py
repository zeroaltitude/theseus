"""The progression and the container agree on the date (theseus-kyrgn).

    python3 -m unittest discover -s bench/recall

The progression narrates a dated calendar ("It's Sunday, 2026-11-15.") and the arm's clock said another
(Mon 2026-10-26), so the model noticed, said so, and was graded confident-wrong for believing the machine:
22 of the 32 confident-wrong probes in the 10-26 seeds were that, and `date` recall sat at 0.367. These
tests pin the repair: the runner pins the clock to the narrated date and refuses a run whose clock and
narration disagree, and a run that was not pinned has its date probes set aside rather than scored.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import clock  # noqa: E402
import generate  # noqa: E402
import progression as pg  # noqa: E402
import score  # noqa: E402
from test_score import PLAN, write_run  # noqa: E402

OPENER = re.compile(r"^It's (\w+), (\d{4}-\d{2}-\d{2})\.")


def smoke(seed: int = 2) -> pg.Progression:
    return generate.build(seed, "smoke", PLAN)


def obedient(env: dict) -> str:
    """A machine whose clock does what the pin says (what libfaketime does)."""
    spec = Path(env["FAKETIME_TIMESTAMP_FILE"]).read_text().strip()
    return spec.lstrip("@").split()[0]


def contradicting(env: dict) -> str:
    """The machine of the 10-26 seeds: its own date, whatever was narrated."""
    return "2026-10-26"


class Plan(unittest.TestCase):
    def test_the_plan_names_each_sessions_narrated_date(self):
        for seed in (2, 7, 11):
            for size in ("smoke", "full"):
                prog = generate.build(seed, size, PLAN)
                plan = pg.clock_plan(prog)
                self.assertEqual(len(plan), len(prog.sessions))
                self.assertGreaterEqual(len(plan), 2)
                openers = [t for t in prog.turns if t.role == "opener"]
                self.assertEqual(len(openers), len(plan))
                for stamp, s, t in zip(plan, prog.sessions, openers):
                    narrated = OPENER.match(t.text)
                    self.assertIsNotNone(narrated, t.text[:60])
                    self.assertEqual(stamp.split()[0], narrated.group(2))
                    self.assertEqual(stamp.split()[0], s.date)

    def test_a_tampered_opener_fails_validation(self):
        prog = smoke()
        opener = next(t for t in prog.turns if t.role == "opener")
        opener.text = opener.text.replace(prog.sessions[0].date, "2026-11-30", 1)
        with self.assertRaises(pg.Invalid) as cm:
            pg.validate(prog)
        self.assertIn("2026-11-30", str(cm.exception))

    def test_a_weekday_that_is_not_the_dates_fails_validation(self):
        prog = smoke()
        prog.sessions[0].weekday = "Thursday" if prog.sessions[0].weekday != "Thursday" else "Friday"
        with self.assertRaises(pg.Invalid):
            pg.validate(prog)

    def test_a_loaded_progression_of_the_published_seeds_still_validates(self):
        # the plan reads dates the 10-18 and 10-26 progressions already carry: no schema change
        for seed in (7, 11, 13):
            pg.validate(generate.build(seed, "full", PLAN))


class Pin(unittest.TestCase):
    def clock(self, tmp, prog, mode="pin", runner=obedient, lib="/x/libfaketime.so.1", **kw):
        return clock.Clock(prog, mode=mode, lib=lib, workdir=Path(tmp), runner=runner, **kw)

    def test_the_machine_reads_the_narrated_date_in_every_session(self):
        prog = generate.build(11, "full", PLAN)
        with tempfile.TemporaryDirectory() as t:
            c = self.clock(t, prog)
            seen = [c.enter(i) for i in range(len(prog.sessions))]
        self.assertEqual(len(seen), 3)
        for rec, s in zip(seen, prog.sessions):
            self.assertEqual(rec["narrated"], s.date)
            self.assertEqual(rec["machine"], s.date)
        self.assertTrue(c.meta["pinned"])
        self.assertEqual([r["machine"] for r in c.meta["sessions"]], [s.date for s in prog.sessions])

    def test_a_machine_that_contradicts_the_narration_refuses_and_says_both_dates(self):
        prog = smoke()
        with tempfile.TemporaryDirectory() as t:
            c = self.clock(t, prog, runner=contradicting)
            with self.assertRaises(clock.ClockDrift) as cm:
                c.enter(0)
        msg = str(cm.exception)
        self.assertIn("2026-10-26", msg)
        self.assertIn(prog.sessions[0].date, msg)

    def test_a_clock_that_drifts_after_the_first_session_is_caught_at_the_next(self):
        prog = generate.build(11, "full", PLAN)
        calls = []

        def drifting(env):
            calls.append(1)
            return obedient(env) if len(calls) < 2 else "2026-12-25"

        with tempfile.TemporaryDirectory() as t:
            c = self.clock(t, prog, runner=drifting)
            c.enter(0)
            with self.assertRaises(clock.ClockDrift):
                c.enter(1)

    def test_the_arms_env_carries_the_preload_and_the_spec_file(self):
        prog = smoke()
        with tempfile.TemporaryDirectory() as t:
            c = self.clock(t, prog)
            env = c.env({"PATH": "/bin", "LD_PRELOAD": "/other.so"})
            c.enter(0)
            spec = Path(env["FAKETIME_TIMESTAMP_FILE"]).read_text().strip()
        self.assertEqual(env["LD_PRELOAD"], "/x/libfaketime.so.1:/other.so")
        self.assertEqual(env["FAKETIME_NO_CACHE"], "1")
        self.assertEqual(spec, f"@{prog.sessions[0].date} {generate_time()}")

    def test_a_run_longer_than_the_pinned_day_refuses_rather_than_roll_past_midnight(self):
        prog = smoke()
        ticks = iter([0.0, 3600.0 * 17])
        with tempfile.TemporaryDirectory() as t:
            c = self.clock(t, prog, now=lambda: next(ticks))
            c.enter(0)
            with self.assertRaises(clock.ClockDrift) as cm:
                c.enter(1)
        self.assertIn("midnight", str(cm.exception))


def generate_time() -> str:
    return pg.CLOCK_TIME


class Modes(unittest.TestCase):
    def test_off_runs_unpinned_and_says_why(self):
        with tempfile.TemporaryDirectory() as t:
            c = clock.Clock(smoke(), mode="off", lib=None, workdir=Path(t))
            self.assertEqual(c.env({"A": "1"}), {"A": "1"})
            c.enter(0)
        self.assertFalse(c.meta["pinned"])
        self.assertIn("--clock off", c.meta["reason"])

    def test_auto_with_no_library_runs_unpinned_loudly(self):
        with tempfile.TemporaryDirectory() as t:
            c = clock.Clock(smoke(), mode="auto", lib=None, workdir=Path(t))
        self.assertFalse(c.meta["pinned"])
        self.assertIn("libfaketime", c.meta["reason"])

    def test_pin_with_no_library_refuses_to_run(self):
        with tempfile.TemporaryDirectory() as t:
            with self.assertRaises(SystemExit) as cm:
                clock.Clock(smoke(), mode="pin", lib=None, workdir=Path(t))
        self.assertIn("libfaketime", str(cm.exception))

    def test_auto_with_a_library_pins(self):
        with tempfile.TemporaryDirectory() as t:
            c = clock.Clock(smoke(), mode="auto", lib="/x/lib.so", workdir=Path(t), runner=obedient)
        self.assertTrue(c.meta["pinned"])


class WhichLibrary(unittest.TestCase):
    """One of the arms is `theseusd`, so only the pthreads build may be preloaded (theseus-h1z44):
    the plain build's unsynchronized static state froze a daemon's clock to 0.392 faked seconds
    across 262 real seconds, and the phase then died on the driver's real-time bounds minutes later,
    with nothing in the daemon's own log to say why."""

    def dirs(self, *names: str) -> Path:
        d = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, d, True)
        for n in names:
            (d / n).write_text("")
        old = clock.LIB_DIRS
        clock.LIB_DIRS = (str(d),)
        self.addCleanup(setattr, clock, "LIB_DIRS", old)
        had = os.environ.pop(clock.LIB_ENV, None)
        if had is not None:
            self.addCleanup(os.environ.__setitem__, clock.LIB_ENV, had)
        return d

    def test_the_thread_safe_build_is_the_one_found(self):
        d = self.dirs(clock.PLAIN_LIB, clock.MT_LIB)
        self.assertEqual(clock.find_lib(), str(d / clock.MT_LIB))

    def test_a_machine_with_only_the_plain_build_has_none(self):
        d = self.dirs(clock.PLAIN_LIB)
        self.assertIsNone(clock.find_lib(), "the build that freezes a daemon's clock is never chosen")
        self.assertEqual(clock.plain_lib(), str(d / clock.PLAIN_LIB))
        self.assertIn(clock.MT_LIB, clock.no_lib_reason())

    def test_a_machine_with_neither_says_so_plainly(self):
        self.dirs()
        self.assertIsNone(clock.find_lib())
        self.assertIsNone(clock.plain_lib())
        self.assertIn("no libfaketime found", clock.no_lib_reason())

    def test_pin_on_a_machine_with_only_the_plain_build_refuses_and_names_the_repair(self):
        self.dirs(clock.PLAIN_LIB)
        with tempfile.TemporaryDirectory() as t:
            with self.assertRaises(SystemExit) as cm:
                clock.Clock(smoke(), mode="pin", lib=clock.find_lib(), workdir=Path(t))
        self.assertIn(clock.MT_LIB, str(cm.exception))


class Live(unittest.TestCase):
    """With a real libfaketime the machine's own `date` is what is read."""

    def test_the_real_machine_reads_the_narrated_date(self):
        lib = clock.find_lib()
        if lib is None:
            self.skipTest("no libfaketime on this machine (set LIBFAKETIME_LIB)")
        prog = generate.build(11, "full", PLAN)
        with tempfile.TemporaryDirectory() as t:
            c = clock.Clock(prog, mode="pin", lib=lib, workdir=Path(t))
            seen = [c.enter(i) for i in range(len(prog.sessions))]
        self.assertEqual([r["machine"] for r in seen], [s.date for s in prog.sessions])
        self.assertTrue(all(r["machine"] != os.popen("date +%F").read().strip() for r in seen))


class DateProbesOfAnUnpinnedRun(unittest.TestCase):
    def date_probes(self, prog):
        return [p for p in prog.probes if p.value_kind == "date"]

    def score(self, meta):
        prog = smoke(2)
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "run"
            write_run(d, prog, "theseus", [], meta=meta)
            (d / "delivered.json").write_text(json.dumps(
                {f.id: {"delivered": True, "marker": f.marker, "turn": f.turn} for f in prog.facts}))
            run = score.load_run(d)
            rows = score.score_run(run)
        return prog, rows

    def test_a_run_with_no_clock_record_sets_its_date_probes_aside(self):
        prog, rows = self.score(None)
        dates = [r for r in rows if r.value_kind == "date"]
        self.assertEqual(len(dates), len(self.date_probes(prog)))
        self.assertGreater(len(dates), 0)
        self.assertEqual({r.status for r in dates}, {"clock_conflict"})
        self.assertEqual(score.summarize(rows)["clock_conflict"], len(dates))

    def test_a_pinned_run_scores_its_date_probes(self):
        prog = smoke(2)
        meta = {"clock": {"pinned": True, "sessions": [{"narrated": x.date, "machine": x.date}
                                                       for x in prog.sessions]}}
        prog, rows = self.score(meta)
        dates = [r for r in rows if r.value_kind == "date"]
        self.assertGreater(len(dates), 0)
        self.assertNotIn("clock_conflict", {r.status for r in dates})
        self.assertEqual(score.summarize(rows)["clock_conflict"], 0)

    def test_a_run_that_says_pinned_but_read_back_another_date_is_not_pinned(self):
        prog = smoke(2)
        meta = {"clock": {"pinned": True, "sessions": [{"narrated": x.date, "machine": "2026-10-26"}
                                                       for x in prog.sessions]}}
        _, rows = self.score(meta)
        self.assertEqual({r.status for r in rows if r.value_kind == "date"}, {"clock_conflict"})

    def test_an_unpinned_run_leaves_every_other_probe_alone(self):
        _, unpinned = self.score(None)
        prog = smoke(2)
        _, pinned = self.score({"clock": {"pinned": True, "sessions": [
            {"narrated": x.date, "machine": x.date} for x in prog.sessions]}})
        other = lambda rows: [(r.probe, r.status, r.correct) for r in rows if r.value_kind != "date"]  # noqa: E731
        self.assertEqual(other(unpinned), other(pinned))
        self.assertGreater(len(other(pinned)), 0)

    def test_a_clock_conflict_is_in_no_accuracy_and_no_confident_wrong(self):
        base = dict(arm="a", probe="p", kind="direct", salience="incidental", carrier="aside", planned="near",
                    bucket="near", turns_since=3, tokens_since=100, cost_usd=0.01, latency_ms=1000)
        ok = score.Scored(value_kind="port", status="scored", correct=True, confident_wrong=False, stale=None,
                          hedged=False, cites_where=None, cites_when=None, **base)
        bad = score.Scored(value_kind="date", status="scored", correct=False, confident_wrong=True, stale=None,
                           hedged=False, cites_where=None, cites_when=None, **base)
        s = score.summarize([ok, bad])
        self.assertEqual((s["recall_accuracy"], s["confident_wrong"], s["clock_conflict"]), (0.5, 1, 0))
        out = score.summarize(score.set_aside_dates([ok, bad]))
        self.assertEqual((out["recall_accuracy"], out["confident_wrong"], out["clock_conflict"]), (1.0, 0, 1))
        self.assertIsNone(out["by_value_kind"]["date"])

    def test_rescoring_a_scores_file_is_the_same_rule(self):
        base = dict(arm="a", probe="p", kind="direct", salience="incidental", carrier="aside", planned="near",
                    bucket="near", turns_since=3, tokens_since=100, cost_usd=0.01, latency_ms=1000,
                    cites_where=None, cites_when=None, stale=None, hedged=False)
        rows = [score.Scored(value_kind="date", status="scored", correct=False, confident_wrong=True, **base),
                score.Scored(value_kind="host", status="scored", correct=True, confident_wrong=False, **base)]
        scores = {"digest": "d", "stale_rule": "strict", "arms": {"a": {"overhead": 7}},
                  "probes": [score.asdict(r) for r in rows]}
        out = score.rescore_clock(scores)
        self.assertEqual(out["arms"]["a"]["clock_conflict"], 1)
        self.assertEqual(out["arms"]["a"]["overhead"], 7)  # what the scorer did not compute is carried
        self.assertEqual([p["status"] for p in out["probes"]], ["clock_conflict", "scored"])
        self.assertEqual(out["clock_rule"], "unpinned-date-probes-set-aside")
        # the original is not touched
        self.assertEqual(scores["probes"][0]["status"], "scored")


if __name__ == "__main__":
    unittest.main()


def host_date() -> str:
    return os.popen("date +%F").read().strip()


class SmokeProof(unittest.TestCase):
    """The pin, shown at smoke level with a control: a process under the pin, the same process without it,
    and the host's own date, side by side. The control is what tells the proof from an empty set."""

    PIN = "2021-03-04"

    def run_smoke(self, runner, host=None, **kw):
        with tempfile.TemporaryDirectory() as t:
            return clock.smoke(self.PIN, lib="/x/libfaketime.so.1", workdir=Path(t), runner=runner,
                               host=host or (lambda: "2026-10-28"), **kw)

    def machine_that_honours_the_pin(self, env: dict) -> str:
        return obedient(env) if "FAKETIME_TIMESTAMP_FILE" in env else "2026-10-28"

    def test_the_pinned_arm_reads_the_pin_and_the_control_reads_the_real_date(self):
        r = self.run_smoke(self.machine_that_honours_the_pin)
        self.assertEqual((r["pinned"], r["control"], r["host"]), (self.PIN, "2026-10-28", "2026-10-28"))
        self.assertTrue(r["proof"])

    def test_a_pin_that_does_nothing_is_not_a_proof(self):
        with self.assertRaises(clock.ClockDrift) as cm:
            self.run_smoke(contradicting)
        self.assertIn(self.PIN, str(cm.exception))

    def test_a_control_that_also_reads_the_pin_is_not_a_proof(self):
        with self.assertRaises(clock.ClockDrift) as cm:
            self.run_smoke(lambda env: self.PIN)
        self.assertIn("control", str(cm.exception))

    def test_a_pin_near_the_real_date_cannot_discriminate(self):
        with self.assertRaises(ValueError):
            self.run_smoke(self.machine_that_honours_the_pin, host=lambda: "2021-03-05")

    def test_the_control_env_carries_no_pin(self):
        seen = []
        self.run_smoke(lambda env: (seen.append(dict(env)), self.machine_that_honours_the_pin(env))[1])
        self.assertEqual(["LD_PRELOAD" in e for e in seen], [True, False])

    def test_the_real_machine_pins_and_its_control_does_not(self):
        lib = clock.find_lib()
        if lib is None:
            self.skipTest("no libfaketime on this machine (set LIBFAKETIME_LIB)")
        with tempfile.TemporaryDirectory() as t:
            r = clock.smoke(self.PIN, lib=lib, workdir=Path(t))
        self.assertEqual(r["pinned"], self.PIN)
        self.assertEqual(r["control"], host_date())
        self.assertNotEqual(r["pinned"], r["control"])

"""The async bench's scorer, over fixture trial directories whose numbers
are worked by hand (theseus-7gir.16).

    python3 -m unittest discover -s bench/async
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import asyncbench as ab  # noqa: E402
import efficiency as ef  # noqa: E402
import score  # noqa: E402

# A sampler's summary of the real shape (sampler.py's), with no cgroup, so
# the work's CPU is what the samples saw.
SUMMARY = {
    "status": "ok", "reason": None, "interval_ms": 250, "samples": 960, "wall_s": 240.0,
    "classes": {
        "harness": {"cpu_s": 4.5, "peak_rss_kb": 76800, "peak_hwm_kb": 61000, "processes": 2},
        "wrapper": {"cpu_s": 0.2, "peak_rss_kb": 3000, "peak_hwm_kb": 2900, "processes": 2},
        "work": {"cpu_s": 31.0, "peak_rss_kb": 400000, "peak_hwm_kb": 300000, "processes": 9},
        "outside": {"cpu_s": 0.3, "peak_rss_kb": 0, "peak_hwm_kb": 0, "processes": 4},
    },
    "cgroup": None,
    "sampler": {"cpu_s": 0.6, "core_share": 0.0025},
}


def record(summary: dict | None) -> dict:
    """bench-efficiency's record, as the arms write it (`efficiency.record`)."""
    return ef.record("theseus", ef.theseus_spend(None, None), summary, wall_s=240.0)


# The fixtures' clocks: the ledger's monotonic time is its wall time less this.
BOOT = 900.0


def iso(t: float) -> str:
    return datetime.fromtimestamp(t, timezone.utc).isoformat().replace("+00:00", "Z")


def ledger(events: list[tuple]) -> str:
    """(kind, tool, step, pid, wall, fields) → a chained ledger's text."""
    prev, lines = ab.GENESIS, []
    for seq, (kind, tool, step, pid, wall, fields) in enumerate(events):
        r = {"seq": seq, "kind": kind, "tool": tool, "step": step, "pid": pid, "ppid": 1,
             "pstart": None, "wall": wall, "mono": wall - BOOT, **fields, "prev": prev}
        r["hash"] = prev = ab.chain_hash(prev, r)
        lines.append(json.dumps(r))
    return "\n".join(lines) + "\n"


def trial(job: Path, name: str, *, arm: str, family: str, start: float, end: float, reward: float,
          events: list[tuple], calls: list[tuple] | None = None, steps: list[tuple] | None = None,
          driver: dict | None = None, problems: list[str] | None = None,
          efficiency: dict | None = None, metadata: dict | None = None) -> None:
    d = job / name
    (d / "agent").mkdir(parents=True)
    (d / "verifier").mkdir()
    (d / "result.json").write_text(json.dumps({
        "task_name": family, "trial_name": name, "agent_info": {"name": arm, "version": "0"},
        "agent_result": {"n_input_tokens": 1000, "n_output_tokens": 100, "cost_usd": 0.05,
                         "metadata": metadata or {}},
        "verifier_result": {"rewards": {"reward": reward}},
        "agent_execution": {"started_at": iso(start), "finished_at": iso(end)},
    }))
    (d / "verifier/ledger.jsonl").write_text(ledger(events))
    (d / "verifier/problems.json").write_text(json.dumps({"family": family, "problems": problems or []}))
    if calls is not None:
        rows = [{"kind": "provider.call", "at_unix_ms": int(at * 1000),
                 "data": {"usage": {"input_tokens": i, "output_tokens": o}}} for at, i, o in calls]
        (d / "agent/theseus-calls.json").write_text(json.dumps({"rows": rows, "total": len(rows)}))
    if steps is not None:
        (d / "agent/trajectory.json").write_text(json.dumps({"steps": [
            {"step_id": n, "source": src, "timestamp": iso(at),
             **({"metrics": {"prompt_tokens": p, "completion_tokens": c}} if p is not None else {})}
            for n, (src, at, p, c) in enumerate(steps, 1)]}))
    if driver is not None:
        (d / "agent/async-driver.json").write_text(json.dumps(driver))
    if efficiency is not None:
        (d / "agent/efficiency.json").write_text(json.dumps(efficiency))


def start(tool, step, pid, wall, duration, **f):
    return ("start", tool, step, pid, wall, {"duration": duration, "scale": 1.0, "sleep": duration, **f})


class Scores(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        theseus, claude = root / "async-theseus", root / "async-claude"
        theseus.mkdir()
        claude.mkdir()
        (theseus / "result.json").write_text("{}")  # the job's own: not a trial
        # Interrupt on Theseus: a 200 s job, the injection 20 s in, its answer 12 s later.
        trial(theseus, "interrupt__aaa", arm="theseus-async", family="interrupt",
              start=1000.0, end=1250.0, reward=1.0,
              events=[start("train-model", "train", 11, 1010.0, 200.0),
                      ("inject", "driver", "inject", 12, 1030.0, {"message": "m", "trigger": {"by": "event"}}),
                      start("ticket-count", "tickets", 13, 1040.0, 2.0),
                      ("end", "ticket-count", "tickets", 13, 1042.0, {"count": 500}),
                      ("end", "train-model", "train", 11, 1210.0, {"score": 8000})],
              # Before the job, two inside it (150 and 300 tokens), and after it.
              calls=[(1005.0, 1000, 10), (1020.0, 100, 50), (1100.0, 250, 50), (1215.0, 9, 9)],
              driver={"family": "interrupt", "injection": {"trigger": {"by": "event"}, "error": None}},
              efficiency=record(SUMMARY))
        # Cancel on Claude Code: its input was closed before the injection.
        trial(claude, "cancel__bbb", arm="claude-code-async", family="cancel",
              start=1000.0, end=1600.0, reward=0.0,
              events=[start("migrate", "migrate", 21, 1010.0, 900.0),
                      ("inject", "driver", "inject", 22, 1025.0, {"message": "m", "trigger": {"by": "event"}})],
              steps=[], problems=["3 of the migration's processes still run"],
              # A record whose sampler never ran: no numbers, not zeros.
              efficiency=record(None),
              driver={"family": "cancel", "injection": {"error": "RuntimeError: not measurable"}})
        # Fanout on Claude Code: part 1 failed once (20 s drawn, 10 run), then 10 s;
        # part 2 took 25 s; part 1's effect twice.
        trial(claude, "fanout__ccc", arm="claude-code-async", family="fanout",
              start=1000.0, end=1100.0, reward=0.0,
              events=[start("ingest", "1", 31, 1001.0, 20.0, attempt=1),
                      start("ingest", "2", 32, 1001.0, 25.0, attempt=1),
                      ("fail", "ingest", "1", 31, 1011.0, {"error": "503"}),
                      start("ingest", "1", 33, 1012.0, 10.0, attempt=2),
                      ("end", "ingest", "1", 33, 1022.0, {"rows": 5}),
                      ("effect", "ingest", "1", 33, 1022.0, {"rows": 5}),
                      ("end", "ingest", "2", 32, 1026.0, {"rows": 7}),
                      ("effect", "ingest", "2", 32, 1026.0, {"rows": 7}),
                      start("ingest", "1", 34, 1030.0, 10.0, attempt=3),
                      ("end", "ingest", "1", 34, 1040.0, {"rows": 6}),
                      ("effect", "ingest", "1", 34, 1040.0, {"rows": 6}),
                      start("ingest", "3", 35, 1041.0, 10.0, attempt=1)],
              # The window is 1001 to 1040: two agent steps inside, one outside, a user step.
              steps=[("user", 1000.0, None, None), ("agent", 1002.0, 10, 5),
                     ("agent", 1039.0, 20, 5), ("agent", 1090.0, 1, 1)],
              driver={"family": "fanout"})
        # Contention on Theseus: 10 + 8 s on acct-north, 12 s on acct-south, 2 at once.
        trial(theseus, "contention__ddd", arm="theseus-async", family="contention",
              start=1000.0, end=1036.0, reward=1.0,
              events=[start("deposit", "acct-north", 41, 1001.0, 10.0, amount=10),
                      start("deposit", "acct-south", 42, 1001.0, 12.0, amount=20),
                      ("end", "deposit", "acct-north", 41, 1011.0, {"amount": 10}),
                      start("deposit", "acct-north", 43, 1012.0, 8.0, amount=5),
                      ("end", "deposit", "acct-south", 42, 1013.0, {"amount": 20}),
                      ("end", "deposit", "acct-north", 43, 1020.0, {"amount": 5})],
              calls=[],
              # No efficiency.json: the record in result.json's metadata, its sampler
              # stopped from outside (`running`), its samples standing.
              metadata={"efficiency": record(dict(SUMMARY, status="running"))})
        cls.scores = {s["trial"]: s for s in (score.score(t) for t in score.trials([theseus, claude]))}
        cls.out = root / "out"
        score.main([str(theseus), str(claude), "--out", str(cls.out)])

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_only_trial_directories_are_read(self):
        self.assertEqual(sorted(self.scores), ["cancel__bbb", "contention__ddd", "fanout__ccc", "interrupt__aaa"])

    def test_interrupt_wall_over_ideal_wait_tax_and_responsiveness(self):
        s = self.scores["interrupt__aaa"]
        self.assertEqual((s["success"], s["ledger_ok"]), (1.0, True))
        self.assertEqual((s["wall_s"], s["ideal_s"], s["over_ideal"]), (250.0, 200.0, 1.25))
        # Only the calls between the job's start (1010) and its end (1210).
        self.assertEqual(s["wait_tax"], {"calls": 2, "tokens": 450, "window_s": 200.0})
        self.assertEqual(s["responsiveness_s"], 12.0)
        self.assertEqual((s["unfinished_steps"], s["duplicated_effects"], s["orphans_alive"]), (0, 0, 0))
        # The record's own fields: harness CPU, its peak RSS (76800 kB), and the work's CPU.
        self.assertEqual((s["harness_cpu_s"], s["harness_peak_rss_mb"], s["work_cpu_s"]), (4.5, 75.0, 31.0))

    def test_an_injection_that_never_arrived_is_not_measurable_not_a_failure(self):
        s = self.scores["cancel__bbb"]
        self.assertEqual((s["measurable"], s["success"], s["responsiveness_s"]),
                         (False, score.NOT_MEASURABLE, score.NOT_MEASURABLE))
        self.assertEqual(s["orphans_alive"], 1)
        self.assertEqual((s["harness_cpu_s"], s["harness_peak_rss_mb"], s["work_cpu_s"]), (None, None, None))

    def test_fanout_ideal_duplicates_and_its_wait_tax_from_the_trajectory(self):
        s = self.scores["fanout__ccc"]
        # Part 1: 20 × 0.5 + 10 + 10 (its third run); part 2: 25; part 3: 10.
        self.assertEqual((s["ideal_s"], s["over_ideal"]), (30.0, 3.333))
        self.assertEqual(s["duplicated_effects"], 1)
        self.assertEqual(s["unfinished_steps"], 1)  # part 3 started and never ended
        self.assertEqual(s["wait_tax"], {"calls": 2, "tokens": 40, "window_s": 39.0})
        self.assertIsNone(s["responsiveness_s"])

    def test_contention_ideal_is_the_services_bound(self):
        s = self.scores["contention__ddd"]
        # max((10 + 12 + 8) / 2, 10 + 8) = 18.
        self.assertEqual((s["ideal_s"], s["over_ideal"]), (18.0, 2.0))
        self.assertEqual(s["wait_tax"], {"calls": 0, "tokens": 0, "window_s": 19.0})
        # The record from result.json's metadata, its sampler `running`.
        self.assertEqual((s["harness_cpu_s"], s["harness_peak_rss_mb"], s["work_cpu_s"]), (4.5, 75.0, 31.0))

    def test_a_record_of_another_shape_or_a_sampler_that_failed_has_no_numbers(self):
        with tempfile.TemporaryDirectory() as d:
            t = Path(d)
            (t / "agent").mkdir()
            for rec in ({"cpu_s": 12.5, "peak_rss_mb": 300.0},
                        record(dict(SUMMARY, status="failed", reason="invented"))):
                (t / "agent/efficiency.json").write_text(json.dumps(rec))
                self.assertEqual(score.efficiency(t, {}), {"harness_cpu_s": None, "harness_peak_rss_mb": None,
                                                           "work_cpu_s": None})

    def test_the_report_has_a_row_per_arm_and_family(self):
        report = (self.out / "report.md").read_text()
        rows = [line for line in report.splitlines() if line.startswith("| ") and "Arm" not in line]
        self.assertEqual([r.split(" | ")[:2] for r in rows],
                         [["| claude-code-async", "cancel"], ["| claude-code-async", "fanout"],
                          ["| theseus-async", "contention"], ["| theseus-async", "interrupt"]])
        cancel = rows[0].split(" | ")
        self.assertEqual(cancel[3], score.NOT_MEASURABLE)
        self.assertIn("| 1/1 | 1.25 | 250 | 200 | 2 | 450 | 12 | 0 | 0 | 4.5 | 75 | 31 | 0.05 |", rows[3])
        self.assertIn("| Harness CPU s | Harness peak RSS MB | Work CPU s | Cost $ |", report)
        self.assertEqual(len(json.loads((self.out / "scores.json").read_text())), 4)


if __name__ == "__main__":
    unittest.main()

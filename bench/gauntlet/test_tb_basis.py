"""The Terminal-Bench cell report refuses a cost ratio across two price tables (theseus-0j5k5).

    python3 -m unittest discover -s bench/gauntlet

Runs `tb_report.py` end to end over a synthetic cell, against the published 10-18 baseline
(billed on the table before `theseus-rko44`), so the refusal and the positive case are both
the program's own output and exit code, not a helper's.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import price_basis as pb  # noqa: E402
import tb_report  # noqa: E402

S55 = "claude-sonnet-5-5"


def trial(job: Path, n: int, table: str, *, stamp: bool = True, cache_read: int = 3_000_000):
    by = {S55: {"input": 5000, "cache_read": cache_read, "cache_write": 80_000,
                "output": 12_000, "calls": 20, "cost_usd": 0.0}}
    eff = {"cost_usd": pb.reprice(by, pb.TABLES[table]), "model_calls": 20, "tool_calls": 18,
           "tokens": {"input": 5000, "cache_read": cache_read, "cache_write": 80_000, "output": 12_000},
           "by_model": by, "build_commit": "abc1234"}
    if stamp:
        eff.update(pb.stamp(eff))
    d = job / f"task-{n}__AAA{n}"
    (d / "verifier").mkdir(parents=True)
    (d / "verifier" / "reward.txt").write_text("1\n")
    (d / "result.json").write_text(json.dumps({
        "task_name": f"task-{n}", "trial_name": d.name,
        "started_at": "2026-10-27T15:00:00Z", "finished_at": "2026-10-27T15:10:00Z",
        "agent_execution": {"started_at": "2026-10-27T15:01:00Z", "finished_at": "2026-10-27T15:09:00Z"},
        "agent_result": {"metadata": {"efficiency": eff}}}))


def cell(root: Path, table: str, n: int = 3, **kw) -> Path:
    run = root / "cell-x"
    job = run / "jobs" / "j1"
    for i in range(n):
        trial(job, i, table, **kw)
    return run


def run_report(run: Path, *args: str):
    out, err = io.StringIO(), io.StringIO()
    argv = ["tb_report.py", str(run), "--json", str(run / "out.json"), *args]
    code = 0
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        old = sys.argv
        sys.argv = argv
        try:
            code = tb_report.main()
        except SystemExit as e:
            code = e.code if isinstance(e.code, int) else 1
            err.write(str(e.code))
        finally:
            sys.argv = old
    return code, out.getvalue() + err.getvalue()


class CostRatio(unittest.TestCase):
    def test_the_published_baseline_is_on_the_old_table(self):
        b = tb_report.baseline("theseus-high-v1")
        self.assertEqual({r["price_digest"] for r in b["rows"]}, {pb.digest_of(pb.PRE_RKO44)})
        self.assertGreater(len(b["rows"]), 100)

    def test_a_cell_on_the_new_table_is_refused_against_it_and_the_exit_is_nonzero(self):
        with tempfile.TemporaryDirectory() as t:
            code, out = run_report(cell(Path(t), pb.CURRENT), "--baseline", "theseus-high-v1")
        self.assertEqual(code, 3, out)
        self.assertIn("refusing a ratio across price tables", out)
        self.assertIn(pb.digest_of(pb.CURRENT), out)
        self.assertIn(pb.digest_of(pb.PRE_RKO44), out)
        self.assertNotIn("$/trial median", out)  # no ratio line was printed

    def test_the_refusal_is_in_the_dataset_too(self):
        with tempfile.TemporaryDirectory() as t:
            run = cell(Path(t), pb.CURRENT)
            run_report(run, "--baseline", "theseus-high-v1")
            data = json.loads((run / "out.json").read_text())
        self.assertIn("refused", data["cost_comparison"])
        self.assertEqual(data["price_digests"], [pb.digest_of(pb.CURRENT)])

    def test_a_cell_on_the_baselines_table_prints_its_ratio_and_exits_zero(self):
        with tempfile.TemporaryDirectory() as t:
            run = cell(Path(t), pb.PRE_RKO44)
            code, out = run_report(run, "--baseline", "theseus-high-v1")
            data = json.loads((run / "out.json").read_text())
        self.assertEqual(code, 0, out)
        self.assertIn("$/trial median", out)
        self.assertIn("$/trial mean", out)
        cmp_ = data["cost_comparison"]
        self.assertNotIn("refused", cmp_)
        self.assertGreater(cmp_["median"]["n"]["cell"], 0)
        self.assertGreater(cmp_["median"]["ratio"], 0)
        self.assertEqual(cmp_["median"]["basis"], pb.digest_of(pb.PRE_RKO44))

    def test_a_cell_with_no_stamp_is_measured_from_its_own_tokens(self):
        with tempfile.TemporaryDirectory() as t:
            run = cell(Path(t), pb.PRE_RKO44, stamp=False)
            code, out = run_report(run, "--baseline", "theseus-high-v1")
        self.assertEqual(code, 0, out)
        self.assertIn("$/trial median", out)

    def test_a_cell_with_no_cache_reads_cannot_show_its_table_and_is_refused(self):
        with tempfile.TemporaryDirectory() as t:
            code, out = run_report(cell(Path(t), pb.CURRENT, stamp=False, cache_read=0),
                                   "--baseline", "theseus-high-v1")
        self.assertEqual(code, 3, out)
        self.assertIn("no price digest", out)

    def test_no_baseline_means_no_ratio_and_no_refusal(self):
        with tempfile.TemporaryDirectory() as t:
            code, out = run_report(cell(Path(t), pb.CURRENT), "--no-baseline")
        self.assertEqual(code, 0, out)
        self.assertNotIn("refusing", out)

    def test_repricing_a_baseline_with_no_per_model_usage_still_refuses(self):
        # the published CSV carries tokens but no model names: its dollars cannot be moved
        with tempfile.TemporaryDirectory() as t:
            code, out = run_report(cell(Path(t), pb.CURRENT), "--baseline", "theseus-high-v1",
                                   "--reprice", pb.CURRENT)
        self.assertEqual(code, 3, out)
        self.assertIn("cannot reprice", out)


if __name__ == "__main__":
    unittest.main()

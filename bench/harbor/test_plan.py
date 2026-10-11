"""Tests for the attempt plan and the run's launcher (theseus-w052 3 and 4),
on a scratch git repository. Standard library only."""

from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import plan as planmod
import quiet
import run as runmod

GIT = ["git", "-c", "user.name=Tester", "-c", "user.email=tester@example.invalid", "-c", "commit.gpgsign=false"]


class Repo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        self.plans = self.repo / "bench" / "plans"
        self.plans.mkdir(parents=True)
        self.git("init", "-q")
        (self.repo / "README").write_text("x\n")
        self.git("add", "README")
        self.git("commit", "-q", "-m", "first")
        self.file = self.plans / "r0.json"
        self.file.write_text(json.dumps({"name": "r0", "k": 3}))

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *a: str) -> str:
        return subprocess.run([*GIT, "-C", str(self.repo), *a], check=True, capture_output=True, text=True).stdout

    def commit_plan(self) -> str:
        self.git("add", str(self.file))
        self.git("commit", "-q", "-m", "plan")
        return self.git("rev-parse", "HEAD").strip()

    def load(self):
        return planmod.load(self.file, self.repo, self.plans)


class Plan(Repo):
    def test_an_uncommitted_plan_is_refused(self):
        with self.assertRaises(planmod.PlanRefused) as c:
            self.load()
        self.assertIn("is not committed", str(c.exception))

    def test_a_staged_but_uncommitted_plan_is_refused(self):
        self.git("add", str(self.file))
        with self.assertRaises(planmod.PlanRefused) as c:
            self.load()
        self.assertIn("differs from its commit", str(c.exception))

    def test_a_committed_plan_edited_afterwards_is_refused_and_a_restored_one_passes(self):
        commit = self.commit_plan()
        self.assertEqual(self.load()["commit"], commit)
        self.file.write_text(json.dumps({"name": "r0", "k": 5}))
        with self.assertRaises(planmod.PlanRefused) as c:
            self.load()
        self.assertIn("differs from its commit", str(c.exception))
        self.git("checkout", "--", str(self.file))
        self.assertEqual(self.load()["k"], 3)

    def test_a_staged_edit_of_a_committed_plan_is_refused_too(self):
        self.commit_plan()
        self.file.write_text(json.dumps({"name": "r0", "k": 4}))
        self.git("add", str(self.file))
        with self.assertRaises(planmod.PlanRefused):
            self.load()

    def test_the_record_names_the_commit_that_holds_the_plan_and_the_head(self):
        commit = self.commit_plan()
        (self.repo / "other").write_text("y\n")
        self.git("add", "other")
        self.git("commit", "-q", "-m", "later")
        head = self.git("rev-parse", "HEAD").strip()
        got = self.load()
        self.assertEqual(got["commit"], commit)
        self.assertEqual(got["head"], head)
        self.assertNotEqual(commit, head)
        self.assertEqual(got["file"], "bench/plans/r0.json")
        self.assertEqual(len(got["sha256"]), 64)

    def test_a_file_outside_bench_plans_is_refused(self):
        stray = self.repo / "plan.json"
        stray.write_text(json.dumps({"name": "r0", "k": 3}))
        self.git("add", "plan.json")
        self.git("commit", "-q", "-m", "stray")
        with self.assertRaises(planmod.PlanRefused) as c:
            planmod.load(stray, self.repo, self.plans)
        self.assertIn("bench/plans", str(c.exception))
        with self.assertRaises(planmod.PlanRefused):
            planmod.load(self.plans / "missing.json", self.repo, self.plans)

    def test_a_plan_must_say_its_attempts_once(self):
        for bad in ({"name": "x"}, {"name": "x", "k": 2, "attempts": {"a": 1}}, {"name": "x", "k": 0},
                    {"name": "x", "attempts": {}}, {"name": "x", "attempts": {"a": 0}}, {"k": 3}, [1]):
            with self.assertRaises(planmod.PlanRefused, msg=bad):
                planmod.check(bad)
        planmod.check({"name": "x", "attempts": {"a": 2}})

    def test_a_per_task_plan_makes_one_run_a_group_of_attempts(self):
        plan = {"name": "n", "attempts": {"fix-git": 2, "build-pmars": 5, "cobol": 2}}
        self.assertEqual(planmod.attempt_groups(plan), [(2, ["cobol", "fix-git"]), (5, ["build-pmars"])])
        lines = planmod.commands(plan, ["harbor", "run", "-d", "tb"], "job")
        self.assertEqual(lines[0], ["harbor", "run", "-d", "tb", "--job-name", "job-k2", "-k", "2",
                                    "-i", "cobol", "-i", "fix-git"])
        self.assertEqual(lines[1][-4:], ["-k", "5", "-i", "build-pmars"])
        one = planmod.commands({"name": "n", "k": 3}, ["harbor", "run"], "job")
        self.assertEqual(one, [["harbor", "run", "--job-name", "job", "-k", "3"]])

    def test_the_command_line_cannot_decide_attempts_or_tasks(self):
        for flag in (["-k", "2"], ["--n-attempts", "2"], ["-i", "fix-git"], ["--n-attempts=2"]):
            with self.assertRaises(planmod.PlanRefused, msg=flag):
                planmod.commands({"name": "n", "k": 3}, ["harbor", "run", *flag], "job")


class Launcher(Repo):
    def quiet_host(self):
        p = self.repo / "cpu"
        p.write_text("some avg10=0.10 avg60=0 avg300=0 total=0\n")
        return str(p)

    def test_an_uncommitted_plan_does_not_start_a_run_and_says_why(self):
        ran = []
        pressure = self.quiet_host()
        with self.assertRaises(runmod.Refused) as c:
            runmod.prepare(str(self.file), ["harbor", "run"], repo=self.repo, plans=self.plans,
                           lock=self.repo / "lock", pressure_file=pressure)
        self.assertIn("is not committed", c.exception.reasons[0])
        self.assertEqual(ran, [])

    def test_a_committed_plan_on_a_quiet_host_gets_its_record_and_four_trials_at_once(self):
        commit = self.commit_plan()
        rec = runmod.prepare(str(self.file), ["harbor", "run", "-o", "out", "--job-name", "r0-theseus"],
                             repo=self.repo, plans=self.plans, lock=self.repo / "lock",
                             pressure_file=self.quiet_host())
        self.assertEqual(rec["plan"]["commit"], commit)
        self.assertEqual(rec["parallel"], 4)
        self.assertEqual(rec["jobs_dir"], "out")
        self.assertEqual(rec["lines"], [["harbor", "run", "-o", "out", "-n", "4", "--job-name", "r0-theseus",
                                         "-k", "3"]])
        self.assertEqual(rec["host"]["cpu_pressure_avg10"], 0.1)

    def test_main_refuses_with_exit_3_and_starts_nothing_on_a_busy_host(self):
        self.commit_plan()
        busy = self.repo / "busy"
        busy.write_text("some avg10=40.00 avg60=0 avg300=0 total=0\n")
        ran = []
        old = (runmod.planmod.REPO, runmod.planmod.PLANS, quiet.PRESSURE_FILE)
        runmod.planmod.REPO, runmod.planmod.PLANS = self.repo, self.plans
        try:
            real = runmod.prepare

            def prepare(plan_file, harbor, **kw):
                return real(plan_file, harbor, repo=self.repo, plans=self.plans, lock=self.repo / "lock",
                            pressure_file=str(busy))

            runmod.prepare = prepare
            code = runmod.main(["--plan", str(self.file), "--", "harbor", "run", "-o", str(self.repo / "jobs")],
                               runner=lambda line: ran.append(line) or 0)
        finally:
            runmod.prepare = real
            runmod.planmod.REPO, runmod.planmod.PLANS, _ = old
        self.assertEqual(code, 3)
        self.assertEqual(ran, [])
        self.assertFalse((self.repo / "jobs").exists())

    def test_main_runs_each_group_and_writes_the_record_beside_the_job(self):
        self.file.write_text(json.dumps({"name": "n", "attempts": {"a": 1, "b": 2}}))
        commit = self.commit_plan()
        ran = []
        real = runmod.prepare
        jobs = self.repo / "jobs"

        def prepare(plan_file, harbor, **kw):
            return real(plan_file, harbor, repo=self.repo, plans=self.plans, lock=self.repo / "lock",
                        pressure_file=self.quiet_host())

        runmod.prepare = prepare
        try:
            code = runmod.main(["--plan", str(self.file), "--", "harbor", "run", "-o", str(jobs),
                                "--job-name", "j"], runner=lambda line: ran.append(line) or 0)
        finally:
            runmod.prepare = real
        self.assertEqual(code, 0)
        self.assertEqual(len(ran), 2)
        rec = json.loads((jobs / "j.run.json").read_text())
        self.assertEqual(rec["plan"]["commit"], commit)
        self.assertEqual(len(rec["lines"]), 2)


if __name__ == "__main__":
    unittest.main()

"""The async bench's tasks, offline (theseus-7gir.16): no Harbor, no Docker.

    python3 -m unittest discover -s bench/async

Each family's oracle (`solution/solve.sh`) runs on this host under a scratch
`ASYNC_ROOT`, its tools on PATH, at a small time scale, and its verifier
(`tests/test.sh`) gives reward 1; a planted wrong effect per family gives 0,
and so does an edited ledger.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
TASKS = HERE / "tasks"
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))

import asyncbench as ab  # noqa: E402
import sync  # noqa: E402

SCALE = "0.01"
FAMILIES = ["parallel", "wait-tax", "interrupt", "fanout", "cancel", "contention"]
# Words that would tell an agent how to run a task, not what it is.
HOW = ["in parallel", "background", "concurrent", "subagent", "subtask", "task.create", "async"]


class Trial:
    """One family's scratch root: its files, the oracle's or a planted
    script run on it, and its verifier's result."""

    def __init__(self, family: str, scale: str = SCALE):
        self.family = family
        self.task = TASKS / family
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)
        (self.root / "app").mkdir()
        if (self.task / "environment/app").is_dir():
            for f in (self.task / "environment/app").iterdir():
                shutil.copy(f, self.root / "app" / f.name)
        self.env = dict(
            os.environ,
            ASYNC_ROOT=str(self.root),
            ASYNC_TIME_SCALE=scale,
            PATH=f"{self.task / 'environment/async/bin'}:{os.environ['PATH']}",
        )

    def run(self, script: str | None = None) -> subprocess.CompletedProcess:
        path = self.task / "solution/solve.sh"
        if script is not None:
            path = self.root / "planted.sh"
            path.write_text("#!/bin/bash\nset -uo pipefail\nAPP=\"$ASYNC_ROOT/app\"\n"
                            f"AWAIT=\"python3 {self.task / 'environment/async/lib/asyncbench.py'} await\"\n"
                            + script)
        return subprocess.run(["bash", str(path)], env=self.env, capture_output=True, text=True,
                              timeout=60)

    def check(self, **env: str) -> dict:
        subprocess.run(["bash", str(self.task / "tests/test.sh")], env=dict(self.env, **env),
                       capture_output=True, text=True, timeout=60, check=True)
        logs = self.root / "logs/verifier"
        reward = json.loads((logs / "reward.json").read_text())["reward"]
        problems = json.loads((logs / "problems.json").read_text())["problems"]
        return {"reward": reward, "problems": problems, "ledger": (logs / "ledger.jsonl").exists()}

    def ledger(self) -> list[dict]:
        return ab.read(self.root / "var/lib/async/ledger.jsonl")

    def close(self) -> None:
        # Whatever a planted script left running.
        for r in self.ledger():
            if r["kind"] == "start" and ab._alive(r["pid"], r.get("pstart")):
                os.kill(r["pid"], signal.SIGKILL)
        self.dir.cleanup()


class Layout(unittest.TestCase):
    def test_every_copy_of_the_library_and_every_wrapper_is_syncs(self):
        for path, text in sync.expected().items():
            self.assertTrue(path.exists(), f"{path}: run bench/async/sync.py")
            self.assertEqual(path.read_text(), text, f"{path}: run bench/async/sync.py")
        self.assertEqual(sorted(sync.FAMILY_TOOLS), sorted(FAMILIES))

    def test_each_family_is_a_harbor_task_whose_instruction_says_what_not_how(self):
        for f in FAMILIES:
            t = TASKS / f
            cfg = tomllib.loads((t / "task.toml").read_text())
            self.assertEqual(cfg["metadata"]["async"]["family"], f)
            self.assertGreater(cfg["agent"]["timeout_sec"], 0)
            self.assertTrue((t / "environment/Dockerfile").read_text().startswith("FROM python:"))
            for script in ("solution/solve.sh", "tests/test.sh"):
                self.assertTrue(os.access(t / script, os.X_OK), f"{f}/{script}")
            for tool in sync.FAMILY_TOOLS[f]:
                self.assertTrue(os.access(t / "environment/async/bin" / tool, os.X_OK), tool)
            texts = [(t / "instruction.md").read_text()]
            inj = cfg["metadata"]["async"].get("injection")
            if inj:
                texts.append(inj["message"])
                self.assertEqual(set(inj), {"message", "after_tool", "after_kind", "delay_s", "at_s"})
            for text in texts:
                for word in HOW:
                    self.assertNotIn(word, text.lower(), f"{f}: {word!r}")

    @unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
    def test_harbor_reads_each_task(self):
        from harbor.models.task.config import TaskConfig

        for f in FAMILIES:
            TaskConfig.model_validate(tomllib.loads((TASKS / f / "task.toml").read_text()))


class Oracles(unittest.TestCase):
    def test_each_oracle_earns_reward_1_and_leaves_its_ledger_for_the_verifier(self):
        for f in FAMILIES:
            with self.subTest(family=f):
                t = Trial(f)
                try:
                    r = t.run()
                    self.assertEqual(r.returncode, 0, r.stderr)
                    got = t.check()
                    self.assertEqual((got["reward"], got["problems"]), (1, []))
                    self.assertTrue(got["ledger"])
                    self.assertEqual(ab.verify_chain(t.ledger()), [])
                finally:
                    t.close()

    def test_the_oracles_overlap_their_slow_steps(self):
        """The oracles run the ideal schedule: parallel's six digests overlap,
        and contention never runs more than 2 deposits, nor 2 on one account.
        A larger time scale, so the steps outlast the processes' starts under
        load."""
        t = Trial("parallel", "0.2")
        try:
            t.run()
            rec = t.ledger()
            starts = [r["mono"] for r in ab.by(rec, "start", "digest")]
            ends = [r["mono"] for r in ab.by(rec, "end", "digest")]
            self.assertLess(max(starts), min(ends))
        finally:
            t.close()
        t = Trial("contention", "0.2")
        try:
            t.run()
            rec = t.ledger()
            spans = [(r["step"], s["mono"], r["mono"]) for s in ab.by(rec, "start", "deposit")
                     for r in ab.by(rec, "end", "deposit") if r["pid"] == s["pid"]]
            self.assertEqual(len(spans), 6)
            # At each start, what runs: never more than 2, nor 2 on one account.
            at_once = [[b for b in spans if b[1] <= a[1] < b[2]] for a in spans]
            self.assertEqual(max(len(n) for n in at_once), ab.DEPOSITS_AT_ONCE)
            for n in at_once:
                self.assertEqual(len({b[0] for b in n}), len(n))
        finally:
            t.close()


class Plants(unittest.TestCase):
    """A planted wrong effect per family, and the problem the check names."""

    def planted(self, family: str, script: str, problem: str, scale: str = SCALE, **env: str) -> None:
        t = Trial(family, scale)
        try:
            t.run(script)
            got = t.check(**env)
            self.assertEqual(got["reward"], 0, got)
            self.assertTrue(any(problem in p for p in got["problems"]), got["problems"])
        finally:
            t.close()

    def test_parallel_with_a_part_missing_from_the_aggregate(self):
        self.planted("parallel", """
for p in alpha bravo charlie delta echo foxtrot; do digest "$p" | sed 's/: / /' >> "$APP/all" & done
wait; grep -v foxtrot "$APP/all" > "$APP/digests.txt"
""", "digests.txt's foxtrot is None")

    def test_wait_tax_with_a_token_guessed_instead_of_waited_for(self):
        self.planted("wait-tax", "echo idx-0000000000 > \"$APP/index-token.txt\"\n",
                     "build-index never finished")

    def test_interrupt_with_the_long_job_abandoned(self):
        self.planted("interrupt", """
train-model > /dev/null 2>&1 & pid=$!
ticket-count | sed -n 's/^open tickets: //p' > "$APP/tickets.txt"
$AWAIT --tool train-model --kind start --timeout 60 > /dev/null; kill -KILL "$pid"
echo 9000 > "$APP/score.txt"
""", "train-model never finished")

    def test_fanout_with_an_effect_twice(self):
        self.planted("fanout", """
for p in 1 2 3 4 5 6; do until ingest "$p" >> "$APP/out"; do :; done & done; wait
until ingest 1 > /dev/null; do :; done
""", "part 1 was ingested twice")

    def test_cancel_with_an_orphan_left_running(self):
        """The parent killed alone: its two workers live on."""
        self.planted("cancel", """
migrate > /dev/null 2>&1 & pid=$!
$AWAIT --tool migrate --kind start --count 3 --timeout 60 > /dev/null; kill -KILL "$pid"; wait "$pid"
""", "2 of the migration's processes still run")

    def test_contention_with_a_lost_update(self):
        """Two deposits to one account, the second started while the first
        sleeps: at this scale a deposit sleeps 8 to 15 s."""
        self.planted("contention", """
deposit acct-north 10 & $AWAIT --tool deposit --kind start --timeout 60 > /dev/null
deposit acct-north 5 & wait
for a in acct-north acct-south acct-east acct-west; do echo "$a $(balance "$a")"; done > "$APP/balances.txt"
""", "acct-north holds", scale="1")

    def test_contention_with_more_calls_at_once_than_the_service_allows(self):
        self.planted("contention", """
deposit acct-north 10 & deposit acct-south 20 &
$AWAIT --tool deposit --kind start --count 2 --timeout 60 > /dev/null
deposit acct-east 30; wait
""", "a violation: more than 2 deposits at once", scale="1")


class Ledger(unittest.TestCase):
    """The check reads the ledger as a record of itself first."""

    def edited(self, edit) -> dict:
        t = Trial("wait-tax")
        try:
            t.run()
            path = t.root / "var/lib/async/ledger.jsonl"
            lines = path.read_text().splitlines()
            path.write_text("\n".join(edit(lines)) + "\n")
            return t.check()
        finally:
            t.close()

    def test_an_edited_value_breaks_the_chain(self):
        def edit(lines):
            r = json.loads(lines[-1])
            r["token"] = "idx-0000000000"
            return lines[:-1] + [json.dumps(r)]

        got = self.edited(edit)
        self.assertEqual(got["reward"], 0)
        self.assertIn("line 1: its hash does not follow the line before it", got["problems"])

    def test_a_dropped_line_breaks_the_chain(self):
        got = self.edited(lambda lines: lines[1:])
        self.assertEqual(got["reward"], 0)
        self.assertIn("line 0: seq 1 is not 0", got["problems"])

    def test_an_end_sooner_than_its_drawn_duration_fails(self):
        """A rewritten ledger with a whole new chain still has to keep the
        steps' durations."""
        def edit(lines):
            recs = [json.loads(line) for line in lines]
            recs[-1]["mono"] = recs[0]["mono"] + 0.001
            prev, out = ab.GENESIS, []
            for r in recs:
                r["prev"] = prev
                r["hash"] = prev = ab.chain_hash(prev, r)
                out.append(json.dumps(r))
            return out

        got = self.edited(edit)
        self.assertEqual(got["reward"], 0)
        self.assertTrue(any("before its" in p for p in got["problems"]), got["problems"])

    def test_steps_run_at_another_time_scale_fail(self):
        t = Trial("wait-tax")
        try:
            t.run()
            got = t.check(ASYNC_TIME_SCALE="1")
            self.assertEqual(got["reward"], 0)
            self.assertIn("index: ran at time scale 0.01, not 1.0", got["problems"])
        finally:
            t.close()


if __name__ == "__main__":
    unittest.main()

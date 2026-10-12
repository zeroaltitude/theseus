"""The async bench's kill family, offline (theseus-7ajc): no Harbor, no Docker.

    python3 -m unittest discover -s bench/async

The injection's trigger and seed, each arm's restart command, the scorer's
columns on hand-made ledgers (a duplicate, an orphan, a lost step, a clean
resume), a chain that still verifies with a `kill` line in it, each kill
family's oracle (it resumes, finishes with reward 1 and no duplicate), and a
deliberately naive stand-in harness that re-runs every step after its
restart, killed and restarted through the driver, whose duplicates and
orphans the columns find.
"""

from __future__ import annotations

import asyncio
import json
import os
import shlex
import sys
import tempfile
import time
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import asyncbench as ab  # noqa: E402
import driver  # noqa: E402
import families  # noqa: E402
import layer2 as l2  # noqa: E402
import score  # noqa: E402
import sync  # noqa: E402
import test_tasks as tt  # noqa: E402
from test_driver import FakeEnv, LIB  # noqa: E402

KILL_TASKS = HERE / "kill-tasks"
NAMES = [f.name for f in families.KILL_FAMILIES]
# A step at this scale lasts 0.5 to 2 s: a kill 0.1 s into one lands inside it.
SCALE = "0.05"


class KillTrial(tt.Trial):
    """A kill family's scratch root: its image's marker (the tools then write
    their effect records) and its task directory."""

    def __init__(self, family: str, scale: str = SCALE, task: Path | None = None):
        super().__init__(family, scale)
        self.task = task or KILL_TASKS / family
        # Tools come from the kill task's image, as its Dockerfile COPYs them.
        self.env["PATH"] = f"{self.task / 'environment/async/bin'}:{os.environ['PATH']}"
        (self.root / "var/lib/async").mkdir(parents=True, exist_ok=True)
        (self.root / "var/lib/async/kill-family").write_text("kill-family\n")
        if (self.task / "environment/app").is_dir():
            import shutil

            shutil.rmtree(self.root / "app")
            shutil.copytree(self.task / "environment/app", self.root / "app")

    def kill_json(self) -> dict:
        return json.loads((self.root / "logs/verifier/kill.json").read_text())


def chained(events: list[dict]) -> list[dict]:
    """Hand-made events (kind, tool, step, pid, pstart, plus fields) as a
    ledger's records: seq, a clock that moves 1 s a line, the sha256 chain."""
    out, prev = [], ab.GENESIS
    for seq, e in enumerate(events):
        r = {"seq": seq, "kind": e["kind"], "tool": e.get("tool", ""), "step": e.get("step", ""),
             "pid": e.get("pid", 0), "ppid": 1, "pstart": e.get("pstart"), "wall": 1000.0 + seq,
             "mono": 100.0 + seq, **{k: v for k, v in e.items() if k not in
                                      ("kind", "tool", "step", "pid", "pstart")}, "prev": prev}
        r["hash"] = prev = ab.chain_hash(prev, r)
        out.append(r)
    return out


def start(tool, step, pid, **f):
    return {"kind": "start", "tool": tool, "step": step, "pid": pid, "pstart": str(pid), "duration": 10.0,
            "scale": 1.0, "sleep": 10.0, **f}


def end(tool, step, pid, **f):
    return {"kind": "end", "tool": tool, "step": step, "pid": pid, "pstart": str(pid), **f}


def effect(tool, step, pid):
    return {"kind": "effect", "tool": tool, "step": step, "pid": pid, "pstart": str(pid)}


KILL = {"kind": "kill", "tool": "driver", "step": "kill", "pid": 900, "pstart": "900",
        "trigger": {"by": "seed"}, "pids": [{"pid": 7}], "settle_s": 2.0}
RESTART = {"kind": "restart", "tool": "driver", "step": "restart", "pid": 901, "pstart": "901",
           "resumable": True}


class Layout(unittest.TestCase):
    def test_six_kill_families_of_effects_that_must_not_repeat(self):
        self.assertEqual(len(NAMES), 6)
        self.assertEqual(sorted(NAMES), sorted(l2.KILL_NAMES))
        self.assertEqual(sorted(f.base for f in families.KILL_FAMILIES), sorted(l2.KILL_BASES))
        for f in families.KILL_FAMILIES:
            plain = families.BY_NAME[f.base]
            # The instruction is the plain family's, word for word: it never says how to run.
            self.assertEqual(f.instruction, plain.instruction)
            self.assertEqual((KILL_TASKS / f.name / "instruction.md").read_text(), plain.instruction)
            self.assertEqual(f.tools, plain.tools)
            # Each has a step whose effect is written, so a repeat is a duplicate the scorer reads.
            tools_text = (HERE / "tools/layer2.py").read_text()
            self.assertTrue(any(f'ab.effect("{t}"' in tools_text or t == "restart" for t in f.tools), f.name)

    def test_the_injection_is_in_each_tasks_metadata_with_its_seed(self):
        seeds = set()
        modes = []
        for f in families.KILL_FAMILIES:
            cfg = tomllib.loads((KILL_TASKS / f.name / "task.toml").read_text())
            self.assertEqual(cfg["metadata"]["async"]["family"], f.name)
            self.assertEqual(cfg["metadata"]["async"]["base"], f.base)
            k = driver.kill_of(KILL_TASKS / f.name)
            self.assertIsNotNone(k)
            self.assertIsNone(driver.injection_of(KILL_TASKS / f.name), "a kill is not a message")
            self.assertEqual((k.after_kind, k.settle_s), ("start", 2.0))
            self.assertLess(0, k.window_lo_s)
            self.assertLess(k.window_lo_s, k.window_hi_s)
            seeds.add(k.seed)
            modes.append(k.mode)
        self.assertEqual(len(seeds), 6)
        self.assertEqual(sorted(set(modes)), ["mid-step", "random"], "some kills land in a step, some between")
        self.assertIsNone(driver.kill_of(tt.TASKS / "cancel"))
        self.assertIsNone(driver.kill_of(tt.TASKS / "suites"))

    def test_the_window_is_a_part_of_the_ideal_wall(self):
        for f in families.KILL_FAMILIES:
            k = driver.kill_of(KILL_TASKS / f.name)
            ideal = families.nominal_ideal(f.base)
            self.assertAlmostEqual(k.window_lo_s, ideal * 0.25, delta=0.1)
            self.assertAlmostEqual(k.window_hi_s, ideal * 0.75, delta=0.1)
            self.assertLess(k.window_hi_s, ideal)

    def test_a_kill_images_tools_write_effects_and_a_plain_images_do_not(self):
        for f in families.KILL_FAMILIES:
            self.assertIn("kill-family", (KILL_TASKS / f.name / "environment/Dockerfile").read_text())
            self.assertNotIn("kill-family", (tt.TASKS / f.base / "environment/Dockerfile").read_text())

    def test_the_plain_families_are_as_they_were(self):
        """A plain family's ledger has no effect line it did not have: its
        tools write theirs only where the marker is."""
        t = tt.Trial("migrations")
        try:
            t.run()
            self.assertEqual([r for r in t.ledger() if r["kind"] == "effect"], [])
            self.assertEqual(t.check()["reward"], 1)
        finally:
            t.close()


class Trigger(unittest.TestCase):
    def k(self, **kw):
        base = dict(seed=7, mode="random", window_lo_s=10.0, window_hi_s=40.0, after_tool="restart",
                    after_kind="start", delay_s=0.3, at_s=30.0, settle_s=0.05)
        return driver.KillInjection(**{**base, **kw})

    def test_a_random_kill_time_is_drawn_from_the_seed_in_the_window(self):
        a, b = self.k(seed=1), self.k(seed=2)
        self.assertEqual(driver.kill_time(a), driver.kill_time(a))
        self.assertNotEqual(driver.kill_time(a), driver.kill_time(b))
        for seed in range(50):
            self.assertTrue(10.0 <= driver.kill_time(self.k(seed=seed)) <= 40.0)
        self.assertNotEqual(driver.kill_time(a, "attempt-2"), driver.kill_time(a))

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.env = FakeEnv(self.root)
        self.slept: list[float] = []

    def tearDown(self):
        self.tmp.cleanup()

    def ledger(self):
        return ab.read(self.root / "var/lib/async/ledger.jsonl")

    def sleeper(self):
        async def sleep(s):
            self.slept.append(s)
        return sleep

    def sleeper_arm(self, restart: str | None = "true"):
        # A process to kill: a sleep that is not the test's own.
        return driver.Arm("standin", driver.procs_command("--cmd 'sleep 4242'", lib=LIB), restart, None,
                          "a stand-in")

    def test_a_random_kill_fires_at_its_seeded_time_and_records_it_first(self):
        k = self.k(seed=5, window_lo_s=0.2, window_hi_s=0.4)
        at = driver.kill_time(k)

        async def go():
            victim = await asyncio.create_subprocess_exec("sleep", "4242")
            got = await driver.fire_kill(k, self.sleeper_arm(), self.env, started=__import__("time").monotonic(),
                                         lib=LIB)
            code = await asyncio.wait_for(victim.wait(), 10)
            return got, code

        got, code = asyncio.run(asyncio.wait_for(go(), 60))
        self.assertEqual(code, -9, "SIGKILL")
        self.assertEqual(got["trigger"], {"by": "seed", "seed": 5, "mode": "random", "at_s": at})
        self.assertGreaterEqual(got["killed_after_s"], at)
        kill = [r for r in self.ledger() if r["kind"] == "kill"][0]
        self.assertEqual(kill["trigger"], got["trigger"])
        self.assertEqual([p["pid"] for p in kill["pids"]], [p["pid"] for p in got["killed"]])
        self.assertEqual(kill["settle_s"], 0.05)
        kinds = [r["kind"] for r in self.ledger()]
        self.assertEqual(kinds, ["kill", "restart"], "the kill is recorded before the restart")
        self.assertEqual(ab.verify_chain(self.ledger()), [], "the chain verifies with a kill line in it")

    def test_a_mid_step_kill_fires_the_delay_after_the_steps_first_start(self):
        k = self.k(mode="mid-step", delay_s=0.4, at_s=30.0, after_tool="digest")

        async def go():
            import time

            began = time.monotonic()
            victim = await asyncio.create_subprocess_exec("sleep", "4242")
            await asyncio.sleep(0.2)
            job = await asyncio.create_subprocess_exec(
                sys.executable, LIB, "tool", "digest", "alpha", env=self.env.env, stdout=asyncio.subprocess.DEVNULL)
            got = await driver.fire_kill(k, self.sleeper_arm(), self.env, started=began, lib=LIB)
            await victim.wait()
            await job.wait()
            return got

        got = asyncio.run(asyncio.wait_for(go(), 60))
        st = [r for r in self.ledger() if r["kind"] == "start"][0]
        kill = [r for r in self.ledger() if r["kind"] == "kill"][0]
        self.assertEqual(got["trigger"]["by"], "event")
        self.assertEqual((got["trigger"]["tool"], got["trigger"]["seq"], got["trigger"]["delay_s"]),
                         ("digest", st["seq"], 0.4))
        self.assertEqual(got["trigger"]["mode"], "mid-step")
        self.assertGreaterEqual(kill["mono"], st["mono"] + 0.4)

    def test_with_nothing_to_kill_the_trial_is_not_measurable_and_no_kill_is_recorded(self):
        k = self.k(window_lo_s=0.0, window_hi_s=0.01)
        arm = driver.Arm("standin", driver.procs_command("--cmd 'sleep 31337'", lib=LIB), "true", None, "")
        got = asyncio.run(driver.fire_kill(k, arm, self.env, started=0.0, lib=LIB, sleep=self.sleeper()))
        self.assertIn("not measurable", got["error"])
        self.assertEqual([r for r in self.ledger() if r["kind"] == "kill"], [])

    def test_pid_1_and_pids_not_found_are_never_signalled(self):
        class Env(FakeEnv):
            async def exec(self, command, cwd=None, env=None, timeout_sec=None):
                if "procs" in command:
                    return type("R", (), {"stdout": json.dumps([{"pid": 1}, {"pid": 0}]), "stderr": "",
                                          "return_code": 0})()
                return await super().exec(command, cwd, env, timeout_sec)

        env = Env(self.root)
        got = asyncio.run(driver.fire_kill(self.k(window_lo_s=0, window_hi_s=0.01), self.sleeper_arm(), env,
                                           started=0.0, lib=LIB, sleep=self.sleeper()))
        self.assertIn("not measurable", got["error"])
        self.assertFalse([c for c in env.commands if c.startswith("kill -KILL")])

    def test_an_arm_that_cannot_continue_is_scored_not_resumable_and_never_crashes(self):
        k = self.k(window_lo_s=0.0, window_hi_s=0.01)

        async def go():
            victim = await asyncio.create_subprocess_exec("sleep", "4242")
            got = await driver.fire_kill(k, self.sleeper_arm(None), self.env, started=0.0, lib=LIB,
                                         sleep=self.sleeper())
            await victim.wait()
            return got

        got = asyncio.run(asyncio.wait_for(go(), 60))
        self.assertIs(got["resumable"], False)
        self.assertIn("not resumable", got["error"])
        restart = [r for r in self.ledger() if r["kind"] == "restart"][0]
        self.assertIs(restart["resumable"], False)
        self.assertEqual(asyncio.run(driver.wait_restarted(self.env, got, 0.0)), "none")
        self.assertEqual(self.slept, [k.settle_s])


class Restarts(unittest.TestCase):
    """Each arm's restart command: where its documentation says to continue."""

    def test_theseus_starts_the_daemon_again_and_reattaches_the_cli(self):
        arm = driver.theseus_arm("/opt/theseus", "/state", "/logs", "ses_invented")
        self.assertIn("/opt/theseus/theseusd", arm.restart)
        self.assertIn("THESEUS_SOCKET=/state/theseus.sock", arm.restart)
        self.assertIn("health", arm.restart)
        self.assertIn("wait ses_invented --until settled", arm.restart)
        self.assertNotIn(" ask ", arm.restart, "the daemon resumes its turn: no message is sent")
        self.assertIn("--pidfile /state/theseusd.pid", arm.find)
        self.assertIn("--comm theseus", arm.find)
        self.assertIn("kill -0", driver.alive_command(arm.pidfile))

    def test_claude_code_continues_its_conversation_with_the_flags_it_ran_with(self):
        parts = driver.claude_parts(
            'export PATH="$HOME/.local/bin:$PATH"; printf "%s" "$X" | claude --verbose '
            "--output-format=stream-json --permission-mode=bypassPermissions --print 2>&1 | tee /logs/c.txt")
        arm = driver.claude_arm(parts[1], "/logs/c.txt", parts[0])
        self.assertIn("claude --verbose --output-format=stream-json --permission-mode=bypassPermissions "
                      "--continue --print", arm.restart)
        self.assertIn("Continue.", arm.restart)
        self.assertIn("tee -a /logs/c.txt", arm.restart)
        self.assertIn('export PATH="$HOME/.local/bin:$PATH"', arm.restart)
        self.assertIn("--root", arm.find)
        self.assertIn("claude", arm.find)

    def test_pi_continues_its_session_on_the_same_session_dir(self):
        cmd = (". ~/.nvm/nvm.sh; pi --print --mode json --session-dir /logs/agent/pi/sessions --provider anthropic "
               "--model claude-sonnet-5-5 'do it' 2>&1 </dev/null | grep -v '\"type\":\"message_update\"' | "
               "stdbuf -oL tee /logs/pi.txt")
        parts = driver.pi_parts(cmd)
        arm = driver.pi_arm(parts[1], "/logs/pi.txt", parts[0])
        self.assertIn("pi --print --mode json --session-dir /logs/agent/pi/sessions --provider anthropic "
                      "--model claude-sonnet-5-5 --continue", arm.restart)
        self.assertNotIn("do it", arm.restart, "the instruction is not given again")
        self.assertIn(". ~/.nvm/nvm.sh;", arm.restart)

    def test_opencode_continues_its_last_session_with_its_model_and_flags(self):
        cmd = ("[ -f ~/.nvm/nvm.sh ] && . ~/.nvm/nvm.sh; opencode --model=anthropic/claude-sonnet-5-5 run "
               "--format=json --variant medium --thinking --dangerously-skip-permissions -- 'x' 2>&1 | tee /l/o.txt")
        prefix, model, flags = driver.opencode_parts(cmd)
        arm = driver.opencode_arm(model, flags, "/l/o.txt", prefix)
        self.assertIn("opencode --model=anthropic/claude-sonnet-5-5 run --format=json --variant medium "
                      "--thinking --dangerously-skip-permissions --continue -- Continue.", arm.restart)
        self.assertNotIn("'x'", arm.restart)

    def test_every_restart_returns_at_once_and_records_its_pid(self):
        for arm in (driver.claude_arm("--print-ish ", "/l", ""), driver.pi_arm("", "/l"),
                    driver.opencode_arm("--model=m", "", "/l")):
            self.assertTrue(arm.restart.startswith("nohup sh -c "), arm.name)
            self.assertIn(f"echo $! > {arm.pidfile}", arm.restart)

    def test_the_four_agents_are_wired_to_the_kill(self):
        text = (HERE / "async_agents.py").read_text()
        for needle in ("driver.kill_of", "driver.fire_kill", "driver.theseus_arm", "driver.claude_arm",
                       "driver.pi_arm", "driver.opencode_arm", "class OpenCodeAsync"):
            self.assertIn(needle, text)


class Columns(unittest.TestCase):
    """Each column, on a ledger worked by hand."""

    def test_a_clean_resume_has_no_duplicate_no_orphan_and_no_lost_work(self):
        rec = chained([
            start("migrate-db", "001", 10), end("migrate-db", "001", 10), effect("migrate-db", "001", 10),
            start("migrate-db", "002", 11),            # in flight when it is killed
            KILL, RESTART,
            start("migrate-db", "002", 20), end("migrate-db", "002", 20), effect("migrate-db", "002", 20),
        ])
        self.assertEqual(ab.verify_chain(rec), [])
        c = ab.kill_columns(rec, set())
        self.assertEqual((c["killed"], c["duplicated_effects"], c["orphans"], c["lost_work"], c["resumed"]),
                         (True, 0, 0, 0, True))
        # 100 + 5 is the kill's line, the restarted harness's first step 2 lines after it.
        self.assertEqual(c["time_to_resume_s"], 2.0)

    def test_a_step_whose_effect_is_written_twice_is_a_duplicate(self):
        rec = chained([
            start("restart", "billing", 10), end("restart", "billing", 10), effect("restart", "billing", 10),
            KILL, RESTART,
            start("restart", "billing", 20), end("restart", "billing", 20), effect("restart", "billing", 20),
        ])
        self.assertEqual(ab.kill_columns(rec)["duplicated_effects"], 1)
        self.assertEqual(score.duplicates(rec), 1)

    def test_a_step_done_before_the_kill_and_started_again_is_lost_work(self):
        rec = chained([
            start("convert-export", "jan", 10), end("convert-export", "jan", 10),
            start("convert-export", "feb", 11), end("convert-export", "feb", 11),
            start("convert-export", "mar", 12),
            KILL, RESTART,
            start("convert-export", "jan", 20), end("convert-export", "jan", 20),
            start("convert-export", "feb", 21), end("convert-export", "feb", 21),
            start("convert-export", "mar", 22), end("convert-export", "mar", 22),
        ])
        c = ab.kill_columns(rec)
        self.assertEqual(c["lost_work"], 2, "jan and feb, not mar")
        self.assertEqual(c["duplicated_effects"], 0)

    def test_a_refused_start_is_not_lost_work_and_a_repeat_the_chore_asks_for_is_not_either(self):
        rec = chained([
            start("migrate-db", "001", 10), end("migrate-db", "001", 10),
            KILL, RESTART,
            start("migrate-db", "001", 20, refused=True),
        ])
        self.assertEqual(ab.kill_columns(rec)["lost_work"], 0)
        rec = chained([
            start("health", "mail", 10), end("health", "mail", 10), start("restart", "mail", 11),
            KILL, RESTART,
            start("restart", "mail", 20), end("restart", "mail", 20),
            start("health", "mail", 21), end("health", "mail", 21),
        ])
        self.assertEqual(ab.kill_columns(rec)["lost_work"], 1, "unless told the check follows the restart")
        self.assertEqual(ab.kill_columns(rec, repeats=l2.KILL_REPEATS["health-restart"])["lost_work"], 0)

    def test_a_process_from_before_the_kill_still_running_at_the_end_is_an_orphan(self):
        rec = chained([start("build", "build", 10), KILL, RESTART,
                       start("build", "build", 20), end("build", "build", 20)])
        self.assertEqual(ab.kill_columns(rec, set())["orphans"], 0, "dead with the harness: no orphan")
        c = ab.kill_columns(rec, {(10, "10")})
        self.assertEqual((c["orphans"], c["orphan_pids"]), (1, [10]))

    def test_an_old_process_that_writes_after_the_restart_started_the_step_again_is_an_orphan(self):
        rec = chained([
            start("restart", "billing", 10), KILL, RESTART,
            start("restart", "billing", 20),
            end("restart", "billing", 10), effect("restart", "billing", 10),
            end("restart", "billing", 20), effect("restart", "billing", 20),
        ])
        c = ab.kill_columns(rec, set())
        self.assertEqual((c["orphans"], c["duplicated_effects"]), (1, 1))

    def test_a_job_the_harness_kept_across_the_kill_is_adopted_not_an_orphan(self):
        """Theseus's way: the job survives, ends after the restart, and the
        harness never starts the step again."""
        rec = chained([
            start("build-lib", "core", 10), KILL, RESTART,
            end("build-lib", "core", 10),
            start("link-app", "link", 20), end("link-app", "link", 20),
        ])
        c = ab.kill_columns(rec, set())
        self.assertEqual((c["orphans"], c["lost_work"], c["duplicated_effects"]), (0, 0, 0))
        self.assertEqual(c["time_to_resume_s"], 3.0, "from the kill to the restarted harness's own line, not the job's")

    def test_a_harness_that_never_resumes_has_no_time_to_resume(self):
        c = ab.kill_columns(chained([start("build", "build", 10), KILL, RESTART]), set())
        self.assertEqual((c["resumed"], c["time_to_resume_s"]), (False, None))

    def test_a_ledger_with_no_kill_has_no_kill_columns(self):
        c = ab.kill_columns(chained([start("build", "build", 10), end("build", "build", 10)]))
        self.assertEqual((c["killed"], c["orphans"], c["lost_work"]), (False, None, None))

    def test_wilson_intervals(self):
        lo, hi = score.wilson(5, 10)
        self.assertAlmostEqual(lo, 0.2366, places=3)
        self.assertAlmostEqual(hi, 0.7634, places=3)
        self.assertAlmostEqual(score.wilson(0, 10)[1], 0.2775, places=3)
        self.assertAlmostEqual(score.wilson(10, 10)[0], 0.7225, places=3)
        self.assertIsNone(score.wilson(0, 0))


class Oracles(unittest.TestCase):
    def test_each_kill_oracle_resumes_finishes_with_reward_1_and_no_duplicate(self):
        for f in NAMES:
            with self.subTest(family=f):
                t = KillTrial(f)
                try:
                    r = t.run(timeout=120)
                    self.assertEqual(r.returncode, 0, r.stderr)
                    self.assertEqual(t.left(), [])
                    got = t.check()
                    self.assertEqual((got["reward"], got["problems"]), (1, []))
                    rec = t.ledger()
                    self.assertEqual(ab.verify_chain(rec), [], "the chain verifies with a kill line in it")
                    self.assertEqual([r["kind"] for r in rec if r["kind"] in ("kill", "restart")],
                                     ["kill", "restart"])
                    self.assertEqual(l2.order_violations(f, rec), [])
                    k = t.kill_json()
                    self.assertEqual((k["completed"], k["duplicated_effects"], k["orphans"], k["lost_work"],
                                      k["resumed"]), (True, 0, 0, 0, True), k)
                    self.assertTrue(any(r["kind"] == "effect" for r in rec), "its effects were written")
                    self.assertGreater(k["time_to_resume_s"], 0)
                    # The kill landed inside a step: one started before it has no end.
                    kill_seq = [r for r in rec if r["kind"] == "kill"][0]["seq"]
                    ended = {(r["pid"], r["step"]) for r in rec if r["kind"] == "end"}
                    self.assertTrue([r for r in rec if r["kind"] == "start" and r["seq"] < kill_seq
                                     and (r["pid"], r["step"]) not in ended], "killed mid-step")
                finally:
                    t.close()


class NaiveStandIn(unittest.TestCase):
    """A deliberately naive harness: the plain oracle's whole script, killed
    by the driver, and run again from its first step on the restart."""

    def run_family(self, base: str, after_tool: str):
        t = KillTrial(f"kill-{base}")
        script = tt.TASKS / base / "solution/solve.sh"
        env = FakeEnv(t.root, {k: v for k, v in t.env.items() if k in ("PATH", "TMPDIR")})
        env.env["ASYNC_TIME_SCALE"] = SCALE
        marker = f"naive-harness-{base}"
        # The harness's own command line carries the marker, as a CLI's does its name.
        arm = driver.Arm(
            "naive", driver.procs_command(f"--cmd {shlex.quote(marker)}", "--not-comm sh,dash", "--root", lib=LIB),
            driver.detached(f"exec bash {shlex.quote(str(script))} {marker}", str(t.root / "naive.pid"),
                            "/dev/null"),
            str(t.root / "naive.pid"), "a stand-in")
        k = driver.KillInjection(seed=1, mode="mid-step", window_lo_s=1, window_hi_s=2, after_tool=after_tool,
                                 after_kind="start", delay_s=0.2, at_s=60, settle_s=0.1)

        async def go():
            import time

            await env.exec(arm.restart, timeout_sec=30)  # the first run
            started = time.monotonic()
            got = await driver.fire_kill(k, arm, env, started=started, lib=LIB)
            ended = await driver.wait_restarted(env, got, time.monotonic() + 120, sleep=lambda s: asyncio.sleep(0.2))
            return got, ended

        got, ended = asyncio.run(asyncio.wait_for(go(), 180))
        return t, got, ended

    def test_a_harness_that_re_runs_every_step_duplicates_effects_and_leaves_orphans(self):
        t, got, ended = self.run_family("health-restart", "restart")
        try:
            self.assertEqual(ended, "ended", got)
            self.assertEqual(got["resumable"], True)
            # Let the orphans finish writing before the check, as they would at a trial's end.
            import time

            time.sleep(3)
            t.check()
            k = t.kill_json()
            self.assertTrue(k["resumed"], k)
            self.assertGreaterEqual(k["duplicated_effects"], 1, k)
            self.assertGreaterEqual(k["orphans"], 1, k)
            self.assertGreaterEqual(k["lost_work"], 1, k)
            self.assertEqual(ab.verify_chain(t.ledger()), [])
        finally:
            t.close()

    def test_the_duplicates_column_reads_the_same_ledger_as_the_verifiers(self):
        t, got, ended = self.run_family("csv-merge", "convert-export")
        try:
            import time

            time.sleep(3)
            t.check()
            k = t.kill_json()
            cols = ab.kill_columns(t.ledger(), {(a[0], a[1]) for a in k["alive"]})
            self.assertEqual(cols["duplicated_effects"], k["duplicated_effects"])
            self.assertGreaterEqual(k["duplicated_effects"], 1)
        finally:
            t.close()


class Scoring(unittest.TestCase):
    """A kill family's trial through the scorer, from fixture directories."""

    def make(self, root: Path, name: str, family: str, events: list[dict], reward: int, alive=(),
             resumable=True):
        d = root / name
        (d / "verifier").mkdir(parents=True)
        (d / "agent").mkdir()
        (d / "verifier/ledger.jsonl").write_text("\n".join(json.dumps(r) for r in chained(events)) + "\n")
        (d / "verifier/problems.json").write_text(json.dumps({"family": family, "problems": []}))
        (d / "verifier/kill.json").write_text(json.dumps({"alive": [list(a) for a in alive]}))
        (d / "result.json").write_text(json.dumps({
            "task_name": family, "agent_info": {"name": "x"},
            "verifier_result": {"rewards": {"reward": reward}}}))
        return d

    def test_a_trial_row_and_the_arms_table_with_wilson_intervals(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        clean = [start("migrate-db", "001", 10), KILL, RESTART,
                 start("migrate-db", "001", 20), end("migrate-db", "001", 20), effect("migrate-db", "001", 20)]
        dup = [start("restart", "mail", 10), end("restart", "mail", 10), effect("restart", "mail", 10), KILL,
               RESTART, start("restart", "mail", 20), end("restart", "mail", 20), effect("restart", "mail", 20)]
        orphan = [start("build", "build", 10), KILL, RESTART, start("build", "build", 20),
                  end("build", "build", 20)]
        a = self.make(root, "a", "kill-migrations", clean, 1)
        b = self.make(root, "b", "kill-health-restart", dup, 1)
        c = self.make(root, "c", "kill-pipeline", orphan, 0, alive=[(10, "10")])
        d = self.make(root, "d", "kill-migrations", [start("migrate-db", "001", 10)], 0)
        rows = [score.score(x, "arm-a") for x in (a, b, c)]
        self.assertEqual([r["kill_completed"] for r in rows], [True, True, False])
        self.assertEqual([r["kill_duplicated"] for r in rows], [0, 1, 0])
        self.assertEqual([r["kill_orphans"] for r in rows], [0, 0, 1])
        self.assertEqual([r["kill_resume_s"] for r in rows], [2.0, 2.0, 2.0])
        self.assertEqual([r["kill_lost_work"] for r in rows], [0, 1, 0], "mail's check is restart's repeat; its restart is not")
        # No kill line: the kill never reached the harness. Not measurable, never a failure.
        nk = score.score(d, "arm-a")
        self.assertFalse(nk["measurable"])
        self.assertEqual(nk["success"], score.NOT_MEASURABLE)
        table = score.kill_rows(rows + [nk])
        self.assertEqual(len(table), 1)
        r = table[0]
        self.assertEqual((r["trials"], r["completed"], r["duplicated_trials"], r["orphan_trials"],
                          r["lost_work_trials"]), (3, 2, 1, 1, 1))
        text = score.kill_markdown(table)
        self.assertIn("2/3 (21–94%)", text)
        self.assertIn("1/3 (6–79%)", text)
        self.assertIn("Time to resume s (median)", text)
        self.assertIn("1 duplicated effects, 1 orphans, 1 steps redone, 3/3 resumed", text)
        self.assertIn("The kill family", score.markdown(score.rows(rows), ["j"], table))
        self.assertNotIn("The kill family", score.markdown(score.rows(rows), ["j"]))

    def test_a_not_resumable_arm_is_scored_and_not_measurable_only_when_nothing_was_killed(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        nores = dict(RESTART, resumable=False)
        d = self.make(Path(tmp.name), "n", "kill-service", [start("start-service", "inventory", 10), KILL, nores],
                      0)
        r = score.score(d, "arm-n")
        self.assertTrue(r["measurable"])
        self.assertEqual((r["kill_completed"], r["kill_resumable"], r["kill_resumed"]), (False, False, False))
        self.assertEqual(score.kill_rows([r])[0]["not_resumable"], 1)

    def test_the_kill_families_ideal_and_overlap_are_their_plain_familys(self):
        rec = chained([start("start-service", "inventory", 10, duration=12.0), KILL, RESTART,
                       start("start-service", "inventory", 20, duration=12.0), end("start-service", "inventory", 20),
                       start("query-service", "stock", 21, duration=15.0), end("query-service", "stock", 21)])
        self.assertEqual(l2.ideal("kill-service", rec), l2.ideal("service", rec))
        self.assertEqual(score.ideal("kill-service", rec, None), 27.0)
        self.assertEqual(score.slow_tools("kill-service"), {"start-service", "query-service"})


E2E = os.environ.get("ASYNC_E2E_BIN")


@unittest.skipIf(not E2E, "set ASYNC_E2E_BIN to a directory with theseus, theseusd and theseus-sim")
class TheseusEndToEnd(unittest.TestCase):
    """The kill-migrations family on this workspace's daemon, its model a
    scripted stand-in (`theseus-sim fake-model --rules`) that asks for the
    three migrations in one `proc.run`: the job goes on in the background past
    `proc_sync_secs`, the driver kills `theseusd` and the CLI inside it, starts
    the daemon again and reattaches, and the trial settles. What the daemon
    does with the interrupted turn and its job is read from the ledger, not
    assumed: the test holds the chain, the kill and restart lines, the outcome
    and that the migrations' effects are not written twice."""

    def test_a_killed_daemon_is_restarted_and_the_trial_settles(self):
        import socket
        import subprocess
        import time

        import theseus_bench as tb

        bins = Path(E2E)
        task = KILL_TASKS / "kill-migrations"
        with tempfile.TemporaryDirectory(prefix="ak") as d:
            d = Path(d)
            root, state, logs = d / "root", d / "st", d / "logs"
            for p in (root / "app", root / "var/lib/async", state, logs):
                p.mkdir(parents=True)
            (root / "var/lib/async/kill-family").write_text("kill-family\n")
            tools = task / "environment/async/bin"
            run = ["env", f"ASYNC_ROOT={root}", "ASYNC_TIME_SCALE=0.2", f"PATH={tools}:{os.environ['PATH']}", "sh", "-c"]
            script = ('for m in 001 002 003; do migrate-db "$m" > "$ASYNC_ROOT/out-$m"; done; '
                      'sed -n \'s/.*schema checksum //p\' "$ASYNC_ROOT/out-003" > "$ASYNC_ROOT/app/schema.txt"')
            rules = [{"when": "Apply the database migrations", "calls": [{"name": "proc_run", "input": {
                "argv": run + [script], "timeout_secs": 600}}]}]
            (d / "rules.json").write_text(json.dumps(rules))
            with socket.socket() as sk:
                sk.bind(("127.0.0.1", 0))
                port = sk.getsockname()[1]
            fake = subprocess.Popen([str(bins / "theseus-sim"), "fake-model", "--addr", f"127.0.0.1:{port}",
                                     "--rules", str(d / "rules.json")], stdout=subprocess.DEVNULL)
            try:
                values = tb.settings("claude-sonnet-5-5", str(root / "app"), proc_sync_secs=1,
                                     api_base=f"http://127.0.0.1:{port}")
                (d / "theseus.toml").write_text(tb.profile(tb.PROFILE.read_text(), values))
                renv = {"THESEUS_CONFIG": str(d / "theseus.toml"), "THESEUS_STATE_DIR": str(state),
                        "ANTHROPIC_API_KEY": "sk-invented", "HOME": str(d)}
                env = FakeEnv(root, renv)
                env.env["ASYNC_TIME_SCALE"] = "0.2"
                trial = driver.Theseus(str(bins), str(state), str(logs))
                kill = driver.KillInjection(seed=7102, mode="mid-step", window_lo_s=1, window_hi_s=2,
                                            after_tool="migrate-db", after_kind="start", delay_s=1.0, at_s=60,
                                            settle_s=0.5)
                got = asyncio.run(asyncio.wait_for(self.trial(env, trial, kill, task, renv), 150))
            finally:
                fake.kill()
                fake.wait()
                if os.environ.get("ASYNC_E2E_KEEP"):
                    import shutil

                    shutil.copytree(d, os.environ["ASYNC_E2E_KEEP"], dirs_exist_ok=True,
                                    ignore=shutil.ignore_patterns("*.sock"))
            first, ended, report = got
            self.assertTrue(report.get("killed"), report)
            self.assertEqual(report["restart_exit"], 0, report)
            self.assertEqual(ended, "settled")
            rec = ab.read(root / "var/lib/async/ledger.jsonl")
            self.assertEqual(ab.verify_chain(rec), [])
            self.assertEqual([r["kind"] for r in rec if r["kind"] in ("kill", "restart")], ["kill", "restart"])
            cols = ab.kill_columns(rec, ab.unfinished_alive(rec))
            print("theseus kill columns:", json.dumps(cols))
            self.assertEqual(cols["duplicated_effects"], 0, cols)
            check = subprocess.run(["bash", str(task / "tests/test.sh")], capture_output=True, text=True,
                                   env=dict(os.environ, ASYNC_ROOT=str(root), ASYNC_TIME_SCALE="0.2"))
            print(check.stdout)

    async def trial(self, env, trial, kill, task, renv):
        started = time.monotonic()
        first = asyncio.create_task(env.exec(
            tb_daemon(trial), env={"THESEUS_BENCH_INSTRUCTION": (task / "instruction.md").read_text()},
            timeout_sec=140))
        session = (await env.exec(trial.session_command())).stdout.strip()
        try:
            arm = driver.theseus_arm(trial.bin_dir, trial.state, trial.logs, session, lib=LIB)
            report = await driver.fire_kill(kill, arm, env, started=started, lib=LIB, restart_env=renv)
            try:
                done = await first
            except Exception as e:  # noqa: BLE001 - the ask died by the kill
                done = e
            ended = await trial.settle(env, started + 120)
        finally:
            await env.exec(trial.finish_script(session))
        return done, ended, report


def tb_daemon(trial):
    import theseus_bench as tb

    return tb.daemon_script(trial.bin_dir, trial.state, trial.logs)


if __name__ == "__main__":
    unittest.main()

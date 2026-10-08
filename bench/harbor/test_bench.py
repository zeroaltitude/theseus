"""Tests for the Harbor adapter's parts (theseus-n88g.3).

    python3 -m unittest discover -s bench/harbor

The standard library runs every test but the ones that need Harbor, which
skip; with Harbor installed (its venv's python), those check the trajectory
against Harbor's own ATIF model, and that the adapter loads.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import stat
import subprocess
import tempfile
import time
import tomllib
import unittest
from pathlib import Path

import sampler as smp
import theseus_atif as atif
import theseus_bench as tb
from test_sampler import stop_samplers

REPO = Path(__file__).resolve().parents[2]
SAMPLER = Path(smp.__file__).resolve()
BASH = shutil.which("bash") or "bash"

try:
    from harbor.models.trajectories import Trajectory
    from harbor.utils.trajectory_utils import compute_model_usage
except ImportError:  # Harbor is not installed: those tests skip.
    Trajectory = None


class Profile(unittest.TestCase):
    def test_a_trial_sets_its_lines_and_the_rest_stays(self):
        system = 'Work on your own.\nUse ["bash", "-lc", "..."] when a shell helps; say "done".'
        values = tb.settings(
            "claude-sonnet-5-5",
            "/app/a task",
            max_loops=50,
            spend_limit_usd=0.4,
            proc_sync_secs=600,
            system=system,
            api_base="http://127.0.0.1:9/invented",
        )
        text = tb.profile(tb.PROFILE.read_text(), values)
        cfg = tomllib.loads(text)
        self.assertEqual(cfg["model"]["model"], "claude-sonnet-5-5")
        self.assertEqual(cfg["profiles"]["bench"]["max_loops"], 50)
        self.assertEqual(cfg["profiles"]["bench"]["system"], system)
        self.assertEqual(cfg["kernel"]["spend_limit_usd"], 0.4)
        self.assertEqual(cfg["tools"]["projects_dir"], "/app/a task")
        self.assertEqual(cfg["tools"]["proc_sync_secs"], 600)
        self.assertEqual(cfg["model"]["api_base"], "http://127.0.0.1:9/invented")
        # What the trial does not set is the profile's.
        self.assertEqual(cfg["secrets"], {"anthropic_api_key": "env:ANTHROPIC_API_KEY"})
        self.assertEqual(cfg["policy"]["enforcement"], "open")
        self.assertEqual(cfg["tools"]["roots"], ["/"])
        self.assertFalse(cfg["web"]["enabled"] or cfg["discord"]["enabled"])
        self.assertNotIn("op://", text)

    def test_a_trial_keeps_the_limits_that_end_it(self):
        # The defaults notify and go on (theseus-usei); a trial keeps the old
        # stops, so it ends at its limit (exit 5) and at its loop cap (exit 8).
        values = tb.settings("claude-sonnet-5-5", "/app", max_loops=7, spend_limit_usd=0.5)
        cfg = tomllib.loads(tb.profile(tb.PROFILE.read_text(), values))
        self.assertEqual(cfg["kernel"]["spend_limit_mode"], "ask")
        self.assertEqual(cfg["model"]["max_loops_mode"], "end")
        self.assertEqual(cfg["profiles"]["bench"]["max_loops_mode"], "end")
        self.assertEqual((tb.ended(5), tb.ended(8)), ("spend_limit", "cut"))

    def test_the_profile_asks_for_the_effort_every_arm_asks_for(self):
        """Theseus's bench profile says `effort = "medium"` (theseus-n6p5): on
        Sonnet 5.5 it would send none, and the model's default is high, where
        Claude Code and Pi send medium. A trial's lines leave it alone."""
        import measure

        self.assertEqual(measure.EFFORT, "medium")
        self.assertEqual(tb.profile_effort(), measure.EFFORT)
        values = tb.settings("claude-sonnet-5-5", "/app")
        self.assertEqual(tb.profile_effort(tb.profile(tb.PROFILE.read_text(), values)), "medium")
        self.assertIsNone(tb.profile_effort('[profiles.bench]\nmodel = "m"\n'))

    def test_a_missing_key_goes_into_its_table_and_a_missing_table_is_added(self):
        text = tb.profile(
            "[model]\nprovider = \"anthropic\"\n\n[tools]\nroots = [\"/\"]\n",
            {("tools", "projects_dir"): "/app", ("kernel", "spend_limit_usd"): 1.0},
        )
        cfg = tomllib.loads(text)
        self.assertEqual(cfg["tools"], {"projects_dir": "/app", "roots": ["/"]})
        self.assertEqual(cfg["kernel"], {"spend_limit_usd": 1.0})


class Routed(unittest.TestCase):
    """The routed arm (theseus-eo3h): Theseus as shipped, Jev picking the
    model per message."""

    def parsed(self, **kw):
        values = tb.settings("claude-sonnet-5-5", "/app", routed=True, **kw)
        text = tb.profile(tb.PROFILE_ROUTED.read_text(), values)
        return text, tomllib.loads(text)

    def test_the_routed_profile_is_the_plain_one_with_the_judge_and_the_table_on(self):
        text, cfg = self.parsed(max_loops=50)
        plain = tomllib.loads(tb.profile(tb.PROFILE.read_text(), tb.settings("claude-sonnet-5-5", "/app", max_loops=50)))
        # Every condition the arms share is the plain profile's.
        for table in ("model", "server", "kernel", "tools", "policy", "sandbox", "discord", "web", "index"):
            self.assertEqual(cfg[table], plain[table], table)
        self.assertEqual(cfg["profiles"]["bench"], plain["profiles"]["bench"])
        # Every profile the table routes to ends at its loop cap, as the plain one does (budgets-notify's join fix,
        # theseus-usei): a profile's mode defaults to notify.
        for name in ("sonnet", "opus", "fable", "haiku", "haikuhi"):
            self.assertEqual(cfg["profiles"][name]["max_loops_mode"], "end", name)
        self.assertEqual(cfg["profiles"]["bench"]["effort"], "medium")
        self.assertNotIn("max_output_tokens", text)
        # The differences: the judge, the routing table, the profiles it names.
        self.assertTrue(cfg["judge"]["enabled"])
        self.assertEqual(cfg["judge"]["key_secret"], "jev_api_key")
        self.assertEqual(cfg["routing"]["mode"], "live")
        modes = {m: t["profiles"] for m, t in cfg["routing"]["modes"].items()}
        self.assertEqual(modes, {
            "trivial": ["haiku"], "quick": ["haiku"], "chat": [],
            "sophisticated": ["fable", "opus"], "deep_coding": ["opus", "fable"],
            "routine_coding": ["haikuhi", "sonnet"]})
        named = {p for ps in modes.values() for p in ps}
        self.assertLessEqual(named, set(cfg["profiles"]))
        self.assertEqual({n: cfg["profiles"][n]["model"] for n in tb.ROUTED_PROFILES}, {
            "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5", "fable": "claude-fable-5-1",
            "haiku": "claude-haiku-5-5", "haikuhi": "claude-haiku-5-5"})
        self.assertEqual((cfg["profiles"]["haiku"]["effort"], cfg["profiles"]["haikuhi"]["effort"]), ("low", "high"))
        # The trial's loop cap rides every profile the router may pick.
        for name in ("bench", *tb.ROUTED_PROFILES):
            self.assertEqual(cfg["profiles"][name]["max_loops"], 50, name)
        self.assertNotIn("glm", named)
        self.assertNotIn("op://", text)

    def test_the_jev_key_is_a_second_secret_by_reference_and_the_plain_profile_has_none(self):
        text, cfg = self.parsed()
        self.assertEqual(cfg["secrets"], {"anthropic_api_key": "env:ANTHROPIC_API_KEY",
                                          "jev_api_key": f"env:{tb.JEV_KEY_ENV}"})
        plain = tb.PROFILE.read_text()
        self.assertNotIn(tb.JEV_KEY_ENV, plain)
        self.assertNotIn("[judge", plain)
        self.assertNotIn("[routing", plain)
        self.assertNotIn("jev", plain.lower().replace("jev's", ""))

    def test_the_efforts_the_config_names(self):
        import efficiency as ef

        efforts = ef.profile_efforts(tb.PROFILE_ROUTED.read_text())
        self.assertEqual(efforts["bench"], "medium")
        self.assertEqual((efforts["haiku"], efforts["haikuhi"]), ("low", "high"))
        self.assertEqual((efforts["sonnet"], efforts["opus"], efforts["fable"]), (None, None, None))
        self.assertEqual(tb.profile_effort(path=tb.PROFILE_ROUTED), "medium")

    def test_a_trials_record_adds_jevs_cost_and_names_the_mix(self):
        import efficiency as ef

        rec = {"schema": ef.SCHEMA, "arm": "theseus", "cost_usd": 0.5,
               "by_model": {"claude-haiku-5-5": {"calls": 3}, "claude-opus-5-5": {"calls": 2}}}
        judge = {"rows": [
            {"kind": "judge.call", "data": {"pack": "route.v2", "cost_micros": 1500}},
            {"kind": "judge.call", "data": {"pack": "route.v2", "cost_micros": 500}},
            {"kind": "judge.call", "data": {"pack": "classify.v1", "cost_micros": None}},
        ]}
        routes = {"rows": [
            {"kind": "route.decided", "position": 9, "data": {
                "mode": "deep_coding", "confidence": 0.9, "profile": "opus", "from": "bench",
                "detour": False, "switch": True, "late": False, "reason": "moved"}},
            {"kind": "route.decided", "position": 4, "data": {
                "mode": "quick", "profile": "haikuhi", "from": "bench", "detour": True}},
        ]}
        efforts = {"bench": "medium", "opus": None, "haikuhi": "high"}
        out = ef.routed_record(rec, judge, routes, efforts)
        self.assertEqual(out["arm"], "theseus-routed")
        self.assertEqual((out["model_cost_usd"], out["cost_usd"]), (0.5, 0.502))
        self.assertEqual(out["jev"]["calls"], 3)
        self.assertEqual((out["jev"]["unpriced"], out["jev"]["priced"]), (1, False))
        self.assertEqual(out["jev"]["by_pack"]["route.v2"], {"calls": 2, "cost_usd": 0.002})
        r = out["routing"]
        # Oldest first, by position; efforts from the config.
        self.assertEqual([(t["profile"], t["effort"]) for t in r["turns"]], [("haikuhi", "high"), ("opus", None)])
        self.assertEqual(r["models"], {"claude-haiku-5-5": 3, "claude-opus-5-5": 2})
        self.assertEqual((r["profiles"], r["modes"], r["acted"], r["rows_read"]),
                         ({"haikuhi": 1, "opus": 1}, {"quick": 1, "deep_coding": 1}, 2, True))

    def test_an_unreadable_ledger_is_unknown_not_no_routing_and_an_unpriced_model_stays_so(self):
        import efficiency as ef

        out = ef.routed_record({"cost_usd": None, "by_model": {}}, None, None, {})
        self.assertIsNone(out["cost_usd"])
        self.assertFalse(out["routing"]["rows_read"])
        self.assertEqual(out["routing"]["turns"], [])

    def test_the_key_reaches_only_the_routed_arm(self):
        """The adapter passes the Jev key in the exec's environment for the
        routed arm alone, never in the command or a file, and refuses to start
        without it. Needs Harbor (its venv's python)."""
        try:
            import asyncio
            import theseus_agent
        except ImportError:
            self.skipTest("Harbor is not installed")
        key, seen = "jv-invented-key-5d1e", []

        class Env:
            default_user = None

            async def exec(self, command, **kw):
                return type("R", (), {"stdout": "/app\n", "return_code": 0})()

        async def go(cls, jev):
            with tempfile.TemporaryDirectory() as d:
                agent = cls(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
                uploaded = {}

                async def upload(environment, content, remote_path, filename):
                    uploaded["text"] = content

                async def run(environment, command, env=None, **kw):
                    seen.append((cls.__name__, command, dict(env or {}), uploaded.get("text")))

                agent._upload_config_text = upload
                agent.exec_as_agent = run
                old = {k: os.environ.pop(k, None) for k in (tb.JEV_KEY_ENV,)}
                if jev:
                    os.environ[tb.JEV_KEY_ENV] = key
                try:
                    await agent.run("Do the task.", Env(), None)
                finally:
                    os.environ.pop(tb.JEV_KEY_ENV, None)
                    for k, v in old.items():
                        if v is not None:
                            os.environ[k] = v

        os.environ.setdefault("ANTHROPIC_API_KEY", "an-invented-key")
        asyncio.run(go(theseus_agent.Theseus, True))
        asyncio.run(go(theseus_agent.TheseusRouted, True))
        plain, routed = seen
        self.assertNotIn(tb.JEV_KEY_ENV, plain[2])
        self.assertEqual(routed[2][tb.JEV_KEY_ENV], key)
        for _, command, _, config in seen:
            self.assertNotIn(key, command)
            self.assertNotIn(key, config)
        self.assertIn("[judge]", routed[3])
        self.assertNotIn("[judge]", plain[3])
        self.assertIn("ledger -n 1000 -k judge.call", routed[1])
        self.assertNotIn("ledger", plain[1])
        with self.assertRaises(RuntimeError):
            asyncio.run(go(theseus_agent.TheseusRouted, False))
        # `--ak routed=1` is the same arm.
        with tempfile.TemporaryDirectory() as d:
            agent = theseus_agent.Theseus(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5", routed="1")
            self.assertTrue(agent.routed)
            self.assertEqual(theseus_agent.TheseusRouted.name(), "theseus-routed")


    def test_the_routed_arm_defaults_to_a_spend_limit_that_clears_an_opus_or_fable_reserve(self):
        """The kernel reserves a call's worst case (128,000 output tokens at
        the model's price) before it runs: $2 refuses the first Opus 5.5 call
        (about $2.6) and Fable 5.1 call (about $6.5) the router sends, so the
        routed arm's default is higher, and the plain arm's is still $2.
        THESEUS_BENCH_SPEND_LIMIT wins on either. Needs Harbor."""
        try:
            import asyncio
            import theseus_agent
        except ImportError:
            self.skipTest("Harbor is not installed")
        import tomllib

        class Env:
            default_user = None

            async def exec(self, command, **kw):
                return type("R", (), {"stdout": "/app\n", "return_code": 0})()

        async def limit(cls, env_limit=None):
            with tempfile.TemporaryDirectory() as d:
                agent = cls(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
                got = {}

                async def upload(environment, content, remote_path, filename):
                    got["text"] = content

                async def run(environment, command, env=None, **kw):
                    pass

                agent._upload_config_text = upload
                agent.exec_as_agent = run
                saved = {k: os.environ.get(k) for k in ("THESEUS_BENCH_SPEND_LIMIT", tb.JEV_KEY_ENV)}
                os.environ[tb.JEV_KEY_ENV] = "jv-invented-key-5d1e"
                os.environ.pop("THESEUS_BENCH_SPEND_LIMIT", None)
                if env_limit:
                    os.environ["THESEUS_BENCH_SPEND_LIMIT"] = env_limit
                try:
                    await agent.run("Do the task.", Env(), None)
                finally:
                    for k, v in saved.items():
                        os.environ.pop(k, None)
                        if v is not None:
                            os.environ[k] = v
                return tomllib.loads(got["text"])["kernel"]["spend_limit_usd"]

        os.environ.setdefault("ANTHROPIC_API_KEY", "an-invented-key")
        self.assertEqual(asyncio.run(limit(theseus_agent.Theseus)), 2.0)
        self.assertEqual(asyncio.run(limit(theseus_agent.TheseusRouted)), 20.0)
        self.assertEqual(asyncio.run(limit(theseus_agent.TheseusRouted, "7.5")), 7.5)
        self.assertEqual(asyncio.run(limit(theseus_agent.Theseus, "3")), 3.0)
        # The reserve at 128,000 output tokens, input aside, per model.
        self.assertGreater(128_000 * 20.0 / 1e6, 2.0)   # Opus 5.5
        self.assertGreater(128_000 * 50.0 / 1e6, 2.0)   # Fable 5.1
        self.assertLess(128_000 * 50.0 / 1e6 + 10.0, tb.ROUTED_SPEND_LIMIT_USD)  # a 1M-token prompt


class ExitCodes(unittest.TestCase):
    def test_the_table_is_the_clis(self):
        """ENDS says what `theseus ask` exits with: read from outcome.rs, so
        the two cannot drift."""
        src = (REPO / "crates/theseus/src/outcome.rs").read_text()
        body = src[src.index("pub const fn code(self)"):]
        body = body[: body.index("}\n    }")]
        arms = dict(re.findall(r"Self::(\w+) => (\d+)", body))
        names = {"Done": "done", "Failed": "failed", "SpendLimit": "spend_limit",
                 "Waiting": "waiting", "Refused": "refused", "Cut": "cut", "Stopped": "stopped"}
        self.assertEqual(set(arms), set(names))
        for variant, code in arms.items():
            self.assertEqual(tb.ended(int(code)), names[variant], variant)
        self.assertEqual((tb.ended(130), tb.ended(143), tb.ended(None)),
                         ("signalled", "signalled", "unknown"))


# A stand-in for `theseus`: `ask` reads the instruction, then answers as
# STANDIN_ASK says (done, slow: done after half a second, failed, or wait:
# until a SIGTERM, then stopped);
# `history` prints a session with no nodes.
STANDIN = """#!/bin/sh
case "$*" in
  *" history"*) echo '{"session": {"session_id": "ses_invented"}, "nodes": []}' ;;
  *" ledger "*)
    case "$*" in
      *judge.call*) echo '{"rows": [{"kind": "judge.call", "data": {"pack": "route.v2", "cost_micros": 1200}}], "total": 1}' ;;
      *) echo '{"rows": [{"kind": "route.decided", "position": 7, "data": {"mode": "quick", "profile": "haiku", "from": "bench"}}], "total": 1}' ;;
    esac ;;
  *" ask "*)
    cat > "$STANDIN_DIR/instruction"
    case "$STANDIN_ASK" in
      done) echo '{"stop_reason": "no_tool_calls"}' ;;
      slow) sleep 0.5; echo '{"stop_reason": "no_tool_calls"}' ;;
      failed) echo "theseus: the provider is rate limited (rate_limit_error)" >&2; exit 1 ;;
      wait)
        trap 'echo "{\\"stop_reason\\": \\"stopped\\"}"; exit 9' TERM
        while :; do sleep 0.05; done ;;
    esac ;;
esac
"""


class Scripts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        d = Path(self.tmp.name)
        self.bin, self.state, self.logs = d / "bin", d / "state", d / "logs"
        for p in (self.bin, self.state, self.logs):
            p.mkdir()
        for name in ("theseus", "theseusd"):
            (self.bin / name).write_text(STANDIN)
            (self.bin / name).chmod(stat.S_IRWXU)
        # A sampler a test started is stopped, and waited for, before the
        # directory goes, whatever the test did (theseus-99by).
        self.addCleanup(stop_samplers, self, self.logs, self.state, d)

    def start(self, ask: str, sampler: bool = False, path: str | None = None,
              routed: bool = False) -> subprocess.Popen:
        env = dict(os.environ, STANDIN_DIR=self.tmp.name, STANDIN_ASK=ask,
                   THESEUS_BENCH_INSTRUCTION="Fix the repository's history.")
        if path is not None:
            env["PATH"] = path
        script = tb.run_script(str(self.bin), str(self.state), str(self.logs),
                               str(SAMPLER) if sampler else None, 50, routed)
        p = subprocess.Popen([BASH, "-c", "set -o pipefail; " + script], env=env,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.reap, p)
        return p

    @staticmethod
    def reap(p: subprocess.Popen) -> None:
        """The run's shell, killed and reaped should the test end before it."""
        if p.poll() is None:
            p.kill()
        p.wait(timeout=30)
        for f in (p.stdout, p.stderr):
            f.close()

    def read(self, name: str) -> str:
        return (self.logs / name).read_text()

    def test_a_turn_that_ends_leaves_its_result_history_and_code(self):
        p = self.start("done")
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertEqual(json.loads(self.read(tb.TURN))["stop_reason"], "no_tool_calls")
        self.assertEqual(json.loads(self.read(tb.HISTORY))["nodes"], [])
        self.assertEqual(self.read(tb.EXIT).strip(), "0")
        self.assertTrue((self.logs / tb.DONE).exists())
        instruction = (Path(self.tmp.name) / "instruction").read_text()
        self.assertEqual(instruction, "Fix the repository's history.")

    def test_only_the_routed_run_reads_the_ledger_for_jevs_calls_and_the_routes(self):
        """theseus-eo3h: the routed arm leaves the `judge.call` and
        `route.decided` rows beside the history; the plain arm reads neither."""
        p = self.start("done")
        p.communicate(timeout=30)
        self.assertFalse((self.logs / tb.JUDGE).exists() or (self.logs / tb.ROUTES).exists())
        p = self.start("done", routed=True)
        _, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertEqual(json.loads(self.read(tb.JUDGE))["rows"][0]["data"]["cost_micros"], 1200)
        self.assertEqual(json.loads(self.read(tb.ROUTES))["rows"][0]["data"]["profile"], "haiku")
        self.assertTrue((self.logs / tb.DONE).exists())

    def test_a_failure_says_its_cause_on_stderr_where_harbor_reads_it(self):
        p = self.start("failed")
        _, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 1)
        self.assertIn("rate_limit_error", err)
        self.assertEqual(self.read(tb.EXIT).strip(), "1")
        self.assertTrue((self.logs / tb.HISTORY).exists())

    def test_the_stop_script_stops_the_turn_and_waits_for_the_runs_end(self):
        p = self.start("wait")
        deadline = time.monotonic() + 30
        while not (self.state / "ask.pid").exists() or not (Path(self.tmp.name) / "instruction").exists():
            self.assertLess(time.monotonic(), deadline, "the turn never started")
            time.sleep(0.02)
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs), 10)],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(stop.returncode, 0, stop.stderr)
        # The stop script returns once the run has read the history.
        self.assertTrue((self.logs / tb.DONE).exists())
        p.communicate(timeout=30)
        self.assertEqual(p.returncode, 9)
        self.assertEqual(json.loads(self.read(tb.TURN))["stop_reason"], "stopped")
        self.assertEqual(self.read(tb.EXIT).strip(), "9")

    def summary(self) -> dict:
        return json.loads(self.read(smp.SUMMARY))

    def test_the_sampler_runs_around_the_turn(self):
        p = self.start("slow", sampler=True)
        _, err = p.communicate(timeout=60)
        self.assertEqual(p.returncode, 0, err)
        s = self.summary()
        self.assertEqual((s["status"], s["names"], s["interval_ms"]),
                         ("ok", ["theseus", "theseusd"], 50))
        self.assertGreaterEqual(s["classes"]["harness"]["processes"], 1, "the stand-in is seen")
        self.assertFalse((self.state / "sampler.pid").exists(), "stopped before the done marker")
        self.assertTrue((self.logs / smp.DONE).exists())

    def test_the_stop_script_stops_the_sampler_on_a_timeout(self):
        p = self.start("wait", sampler=True)
        deadline = time.monotonic() + 60
        while not (self.state / "ask.pid").exists():
            self.assertLess(time.monotonic(), deadline, "the turn never started")
            time.sleep(0.02)
        # The run's end now stops the sampler too (at most 5 s more): the
        # first SIGTERM's wait is the adapter's own 20 s, so a starved
        # machine does not reach the second.
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs))],
                              capture_output=True, text=True, timeout=120)
        self.assertEqual(stop.returncode, 0, stop.stderr)
        p.communicate(timeout=30)
        self.assertEqual(p.returncode, 9)
        self.assertEqual(self.summary()["status"], "ok")
        self.assertTrue((self.logs / smp.DONE).exists())
        self.assertFalse((self.state / "sampler.pid").exists())

    def test_the_stop_script_stops_a_sampler_the_run_left(self):
        """The run's own end never came (its shell was killed): the stop
        script still stops the sampler."""
        r = subprocess.run(["sh", "-c", smp.start_script(str(SAMPLER), str(self.logs),
                                                         str(self.state), ["theseus"])],
                           capture_output=True, text=True, timeout=30)
        self.assertEqual(r.returncode, 0, r.stderr)
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs), 1)],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(stop.returncode, 0, stop.stderr)
        self.assertEqual(self.summary()["status"], "ok")
        self.assertTrue((self.logs / smp.DONE).exists())

    def test_an_image_without_python3_still_runs_the_trial(self):
        tools = Path(self.tmp.name) / "tools"
        tools.mkdir()
        for t in ("cat", "rm", "mkdir", "grep", "tail", "touch", "sleep"):
            os.symlink(shutil.which(t), tools / t)
        p = self.start("done", sampler=True, path=str(tools))
        _, err = p.communicate(timeout=60)
        self.assertEqual(p.returncode, 0, err)
        self.assertEqual(json.loads(self.read(tb.TURN))["stop_reason"], "no_tool_calls")
        self.assertEqual(self.summary(), {"status": "unavailable", "reason": "no python3 on PATH"})
        self.assertTrue((self.logs / tb.DONE).exists())

    def test_the_stop_script_with_no_turn_does_nothing(self):
        stop = subprocess.run(["bash", "-c", tb.stop_script(str(self.state), str(self.logs), 1)],
                              capture_output=True, text=True, timeout=30)
        self.assertEqual(stop.returncode, 0, stop.stderr)


# An invented session: the user's message, a model answer with one call, the
# gate's decision, its result, and the closing answer.
HISTORY = {
    "session": {"session_id": "ses_invented1", "cost_usd": 0.03},
    "nodes": [
        {"kind": "user_message", "at_unix_ms": 1791067619162, "text": "Fix the repository."},
        {"kind": "assistant_message", "at_unix_ms": 1791067621537, "loop_index": 0, "text": "",
         "detail": {"model": "claude-sonnet-5-5", "provider": "anthropic", "stop_reason": "tool_use",
                    "cost_usd": 0.02,
                    "usage": {"input_tokens": 100, "output_tokens": 40,
                              "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 500},
                    "tool_calls": [{"id": "toolu_1", "name": "proc_run",
                                    "input": {"argv": ["git", "status"]}}]}},
        {"kind": "tool_call", "at_unix_ms": 1791067621563, "text": "",
         "detail": {"tool_use_id": "toolu_1", "tool": "proc.run",
                    "decision": {"posture": "open", "reason": "enforcement open"}}},
        {"kind": "tool_result", "at_unix_ms": 1791067621728, "text": "[exit code 0]\n## main\n",
         "detail": {"tool_use_id": "toolu_1", "tool": "proc.run", "status": "ok",
                    "is_error": False, "duration_ms": 153}},
        {"kind": "assistant_message", "at_unix_ms": 1791067623000, "loop_index": 1,
         "text": "The repository is fixed.",
         "detail": {"model": "claude-sonnet-5-5", "stop_reason": "end_turn", "cost_usd": 0.01,
                    "usage": {"input_tokens": 50, "output_tokens": 20,
                              "cache_read_input_tokens": 1500}}},
    ],
}


class Trajectories(unittest.TestCase):
    def trajectory(self, turn=None):
        return atif.trajectory(HISTORY, version="0.0.1", model_name="claude-sonnet-5-5", turn=turn)

    def test_each_answer_is_a_step_with_its_calls_results_and_metrics(self):
        t = self.trajectory(turn={"stop_reason": "no_tool_calls", "loops": 2, "tool_calls": 1})
        self.assertEqual([s["step_id"] for s in t["steps"]], [1, 2, 3])
        self.assertEqual([s["source"] for s in t["steps"]], ["user", "agent", "agent"])
        call = t["steps"][1]["tool_calls"][0]
        self.assertEqual((call["tool_call_id"], call["function_name"]), ("toolu_1", "proc_run"))
        self.assertEqual(call["arguments"], {"argv": ["git", "status"]})
        self.assertEqual(call["extra"]["posture"], "open")
        result = t["steps"][1]["observation"]["results"][0]
        self.assertEqual((result["source_call_id"], result["content"]),
                         ("toolu_1", "[exit code 0]\n## main\n"))
        self.assertEqual(t["steps"][1]["metrics"],
                         {"prompt_tokens": 1600, "completion_tokens": 40, "cached_tokens": 1000,
                          "cost_usd": 0.02, "extra": {"cache_creation_input_tokens": 500}})
        self.assertEqual(t["steps"][0]["timestamp"], "2026-10-03T22:46:59.162Z")
        self.assertEqual(t["final_metrics"],
                         {"total_prompt_tokens": 3150, "total_completion_tokens": 60,
                          "total_cached_tokens": 2500, "total_cost_usd": 0.03, "total_steps": 3})
        self.assertEqual(t["extra"], {"stop_reason": "no_tool_calls", "loops": 2, "tool_calls": 1})
        self.assertEqual(t["session_id"], "ses_invented1")

    def test_a_turn_cut_short_keeps_the_spend_of_its_finished_calls(self):
        s = atif.spend(HISTORY)
        self.assertEqual(s, {"cost_usd": 0.03, "input_tokens": 3150, "cache_tokens": 2500,
                             "output_tokens": 60, "calls": 2})
        unpriced = json.loads(json.dumps(HISTORY))
        del unpriced["nodes"][4]["detail"]["cost_usd"]
        self.assertIsNone(atif.spend(unpriced)["cost_usd"])

    def test_a_call_a_stop_cut_is_a_last_step_so_the_totals_are_the_turns(self):
        """A timeout's stop cut a third call before it answered: the turn's
        result counts it at the kernel's estimate, and no node holds it."""
        turn = {"stop_reason": "stopped", "model": "claude-sonnet-5-5", "cost_usd": 0.046,
                "usage": {"input_tokens": 150 + 8066, "output_tokens": 60,
                          "cache_read_input_tokens": 2500, "cache_creation_input_tokens": 500}}
        t = self.trajectory(turn=turn)
        last = t["steps"][-1]
        self.assertEqual((len(t["steps"]), last["source"], last["message"]), (4, "agent", ""))
        self.assertIn("unanswered", last["extra"])
        self.assertEqual(last["metrics"], {"prompt_tokens": 8066, "completion_tokens": 0,
                                           "cached_tokens": 0, "cost_usd": 0.016})
        self.assertEqual((t["final_metrics"]["total_cost_usd"], t["final_metrics"]["total_prompt_tokens"]),
                         (0.046, 3150 + 8066))
        # A turn that its answers account for gets no such step.
        whole = dict(turn, cost_usd=0.03, usage={"input_tokens": 150, "output_tokens": 60,
                                                  "cache_read_input_tokens": 2500,
                                                  "cache_creation_input_tokens": 500})
        self.assertEqual(len(self.trajectory(turn=whole)["steps"]), 3)

    def test_a_result_with_no_call_in_the_history_is_a_system_step(self):
        h = {"session": {}, "nodes": [HISTORY["nodes"][3]]}
        t = atif.trajectory(h, version="0.0.1")
        self.assertEqual([s["source"] for s in t["steps"]], ["system"])
        result = t["steps"][0]["observation"]["results"][0]
        self.assertEqual((result.get("source_call_id"), result["extra"]["tool_use_id"]), (None, "toolu_1"))
        empty = atif.trajectory({"session": {}, "nodes": []}, version="0.0.1")
        self.assertEqual(len(empty["steps"]), 1)

    @unittest.skipIf(Trajectory is None, "Harbor is not installed")
    def test_harbor_reads_it_as_atif_with_the_same_usage(self):
        for t in (self.trajectory(turn={"stop_reason": "no_tool_calls"}),
                  atif.trajectory({"session": {}, "nodes": [HISTORY["nodes"][3]]}, version="x")):
            parsed = Trajectory.model_validate_json(json.dumps(t))
            self.assertEqual(parsed.to_json_dict(), t)
        usage = compute_model_usage(Trajectory.model_validate(self.trajectory()))
        u = usage["claude-sonnet-5-5"]
        self.assertEqual((u.n_input_tokens, u.n_cache_tokens, u.n_output_tokens),
                         (3150, 2500, 60))
        self.assertAlmostEqual(u.cost_usd, 0.03)


@unittest.skipIf(Trajectory is None, "Harbor is not installed")
class Adapter(unittest.TestCase):
    def test_its_record_names_the_effort_asked_and_the_version_the_container_read(self):
        import theseus_agent
        from harbor.models.agent.context import AgentContext

        with tempfile.TemporaryDirectory() as d:
            logs = Path(d)
            (logs / "version.txt").write_text("theseus 0.83.0\n")
            agent = theseus_agent.Theseus(logs_dir=logs, model_name="anthropic/claude-sonnet-5-5")
            ctx = AgentContext()
            agent.populate_context_post_run(ctx)
            rec = json.loads((logs / "efficiency.json").read_text())
            self.assertEqual((rec["effort"], rec["version"], rec["version_asked"]),
                             ("medium", "theseus 0.83.0", None))
            self.assertEqual(ctx.metadata["efficiency"], rec)

    def test_it_loads_as_an_atif_agent_with_an_error_for_each_end(self):
        import theseus_agent

        self.assertEqual(theseus_agent.Theseus.name(), "theseus")
        self.assertTrue(theseus_agent.Theseus.capabilities.atif)
        ends = {code for code, end in tb.ENDS.items() if end not in ("done", "failed", "usage", "unreachable")}
        self.assertEqual(set(theseus_agent.ERRORS), ends)
        self.assertEqual(theseus_agent.SAMPLER, "/installed-agent/bin/sampler.py")


if __name__ == "__main__":
    unittest.main()

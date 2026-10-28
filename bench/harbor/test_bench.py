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
import unittest.mock
from pathlib import Path

import measure as msr
import sampler as smp
import theseus_atif as atif
import efficiency as ef
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

    def test_end_note_off_writes_the_switch(self):
        # `--ak end_note=off` (theseus-53ujg.5): the config says it, and by
        # default it says nothing (the daemon's default is on).
        for ak, off in ((None, False), ("on", False), ("off", True), ("false", True), ("0", True)):
            values = tb.settings("claude-sonnet-5-5", "/app", end_note=tb.end_note_on(ak))
            cfg = tomllib.loads(tb.profile(tb.PROFILE.read_text(), values))
            self.assertEqual(cfg["model"].get("end_note"), False if off else None, ak)
        plain = tomllib.loads(tb.profile(tb.PROFILE.read_text(), tb.settings("claude-sonnet-5-5", "/app")))
        self.assertNotIn("end_note", plain["model"])

    def test_a_trial_keeps_the_limits_that_end_it(self):
        # The defaults notify and go on (theseus-usei); a trial keeps the old
        # stops, so it ends at its limit (exit 5) and at its loop cap (exit 8).
        values = tb.settings("claude-sonnet-5-5", "/app", max_loops=7, spend_limit_usd=0.5)
        cfg = tomllib.loads(tb.profile(tb.PROFILE.read_text(), values))
        self.assertEqual(cfg["kernel"]["spend_limit_mode"], "ask")
        self.assertEqual(cfg["model"]["on_max_loops"], "end")
        self.assertEqual(cfg["profiles"]["bench"]["on_max_loops"], "end")
        self.assertEqual((tb.ended(5), tb.ended(8)), ("spend_limit", "cut"))

    def test_a_trial_sets_its_daemons_day_ceiling_explicitly(self):
        """theseus-l9on: each profile names the day ceiling, so no trial runs
        on the daemon's $200 default; a trial sets it to twice its spend limit,
        above the limit that ends it, or to THESEUS_BENCH_DAY_CEILING."""
        for path in (tb.PROFILE, tb.PROFILE_ROUTED):
            self.assertIn("daily_spend_ceiling_usd", tomllib.loads(path.read_text())["kernel"])
            # theseus-9e02: the ceiling stops a trial's calls; the daemon's default only warns.
            self.assertEqual(tomllib.loads(path.read_text())["kernel"]["daily_spend_ceiling_mode"], "stop")
            for limit in (0.4, 2.0, 20.0):
                values = tb.settings("claude-sonnet-5-5", "/app", spend_limit_usd=limit)
                k = tomllib.loads(tb.profile(path.read_text(), values))["kernel"]
                self.assertEqual(k["daily_spend_ceiling_usd"], 2 * limit)
                self.assertGreater(k["daily_spend_ceiling_usd"], k["spend_limit_usd"])
                self.assertEqual(k["daily_spend_ceiling_mode"], "stop")
            values = tb.settings("claude-sonnet-5-5", "/app", spend_limit_usd=2.0, day_ceiling_usd=7.5)
            k = tomllib.loads(tb.profile(path.read_text(), values))["kernel"]
            self.assertEqual((k["spend_limit_usd"], k["daily_spend_ceiling_usd"]), (2.0, 7.5))
        self.assertEqual(tb.day_ceiling(3.0, {}), 6.0)
        self.assertEqual(tb.day_ceiling(3.0, {tb.DAY_CEILING_ENV: "12.5"}), 12.5)
        for bad in ("0", "-1"):
            with self.assertRaises(ValueError):
                tb.day_ceiling(3.0, {tb.DAY_CEILING_ENV: bad})

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


class Deadline(unittest.TestCase):
    """theseus-5mkz1: the task's timeout, and the deadline the turn is told."""

    def test_the_task_timeout_is_read_from_task_toml_else_900(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(tb.task_timeout(d), 900.0)
            (Path(d) / "task.toml").write_text('[agent]\ntimeout_sec = 3600.0\n')
            self.assertEqual(tb.task_timeout(d), 3600.0)

    def test_the_deadline_is_the_timeout_less_a_minute_and_can_be_turned_off(self):
        env = {k: v for k, v in os.environ.items() if k != tb.DEADLINE_ENV}
        with unittest.mock.patch.dict(os.environ, env, clear=True):
            self.assertEqual(tb.deadline_secs(3600), 3540)
            self.assertIsNone(tb.deadline_secs(90))
            self.assertEqual(tb.deadline_secs(120), 60)
        with unittest.mock.patch.dict(os.environ, {tb.DEADLINE_ENV: "0"}):
            self.assertIsNone(tb.deadline_secs(3600))


class Routed(unittest.TestCase):
    """The routed arm (theseus-eo3h): Theseus as shipped, Jev picking the
    model per message."""

    def test_every_model_the_routed_profile_names_has_a_list_price(self):
        """The as-shipped arm's dollars are priced on the bench's table, as
        every other arm's are: a model Jev may send a message to with no row
        would be priced by Theseus's own table alone."""
        import tomllib

        parsed = tomllib.loads(tb.PROFILE_ROUTED.read_text())
        models = {p["model"] for p in parsed["profiles"].values() if "model" in p}
        models.add(parsed["model"]["model"])
        self.assertGreaterEqual(len(models), 4, models)
        for m in sorted(models):
            self.assertIn(m, ef.LIST_PRICES, f"{m} has no list price row")
        for m in ("claude-opus-5-5", "claude-fable-5-1", "claude-haiku-5-5"):
            by = {m: {"input": 1_000_000, "output": 1_000_000}}
            p = ef.LIST_PRICES[m]
            self.assertAlmostEqual(ef.list_price(by), p["input"] + p["output"])

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
        for name in tb.ROUTED_PROFILES:
            self.assertEqual(cfg["profiles"][name]["on_max_loops"], "end", name)
        self.assertEqual(cfg["profiles"]["bench"]["effort"], "medium")
        self.assertNotIn("max_output_tokens", text)
        # The differences: the judge, the routing table, the profiles it names.
        self.assertTrue(cfg["judge"]["enabled"])
        self.assertEqual(cfg["judge"]["key_secret"], "jev_api_key")
        self.assertEqual(cfg["routing"]["rung"], "live")
        modes = {m: t["profiles"] for m, t in cfg["routing"]["classes"].items()}
        self.assertEqual(modes, {
            "trivial": ["haiku"], "quick": ["haiku"], "chat": [],
            "sophisticated": ["opus", "fable"], "deep_coding": ["sonnethi", "opus"],
            "routine_coding": ["haikuhi", "sonnet"]})
        named = {p for ps in modes.values() for p in ps}
        self.assertLessEqual(named, set(cfg["profiles"]))
        self.assertEqual({n: cfg["profiles"][n]["model"] for n in tb.ROUTED_PROFILES}, {
            "sonnet": "claude-sonnet-5-5", "sonnethi": "claude-sonnet-5-5", "opus": "claude-opus-5-5",
            "opushi": "claude-opus-5-5", "fable": "claude-fable-5-1",
            "haiku": "claude-haiku-5-5", "haikuhi": "claude-haiku-5-5"})
        self.assertEqual({n: cfg["profiles"][n]["effort"] for n in tb.ROUTED_PROFILES}, {
            "sonnet": "medium", "sonnethi": "high", "opus": "medium", "opushi": "high",
            "fable": "high", "haiku": "low", "haikuhi": "high"})
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
        self.assertEqual((efforts["sonnet"], efforts["opus"], efforts["opushi"], efforts["fable"]),
                         ("medium", "medium", "high", "high"))
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


    def test_the_plain_arms_limit_is_two_dollars_plus_one_maximum_reservation(self):
        """A trial that has spent $1.99 must not be refused the call that
        takes it past $2.00 by the reservation alone: the limit is the $2
        budget plus one worst-case reservation at the arm's model (128,000
        output tokens at the output price, and as many input tokens at the
        input price, to a quarter dollar)."""
        capped: dict[str, str] = {}   # no NO_CAP_ENV: the capped run
        self.assertEqual(tb.TRIAL_BUDGET_USD, 2.0)
        self.assertEqual(tb.trial_budget_usd(capped), 2.0)
        self.assertEqual(tb.spend_limit_usd("claude-sonnet-5-5", capped), 3.75)
        self.assertEqual(tb.spend_limit_usd("anthropic/claude-sonnet-5-5", capped), 3.75)
        self.assertEqual(tb.SPEND_LIMIT_USD, 3.75)
        self.assertEqual(tb.spend_limit_usd("claude-opus-5-5", capped), 5.25)
        for m in ("claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1", "claude-haiku-5-5"):
            p = ef.LIST_PRICES[m]
            worst = 128_000 * p["output"] / 1e6 + 128_000 * p["input"] / 1e6
            self.assertGreaterEqual(tb.reservation_usd(m), worst, m)
            self.assertLess(tb.reservation_usd(m), worst + tb.RESERVE_STEP_USD, m)
            # Spent just under the budget, the next call's reserve still fits.
            self.assertGreaterEqual(tb.spend_limit_usd(m, capped), 1.99 + worst, m)
        with self.assertRaises(ValueError):
            tb.spend_limit_usd("no-such-model", capped)

    def test_each_arm_writes_its_day_ceiling_into_the_config_it_uploads(self):
        """theseus-l9on: the adapter's uploaded config, read back, carries the
        day ceiling at twice the trial's limit on both arms, and
        THESEUS_BENCH_DAY_CEILING's where it names one. Needs Harbor."""
        try:
            import asyncio
            import theseus_agent
        except ImportError:
            self.skipTest("Harbor is not installed")

        class Uploaded(Exception):
            pass

        class Env:
            default_user = None

            async def exec(self, command, **kw):
                return type("R", (), {"stdout": "/app\n", "return_code": 0})()

        def kernel(cls, env):
            with tempfile.TemporaryDirectory() as d:
                agent = cls(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
                got = {}

                async def upload(environment, content, remote_path, filename):
                    got["text"] = content
                    raise Uploaded

                agent._upload_config_text = upload
                keys = ("THESEUS_BENCH_SPEND_LIMIT", tb.DAY_CEILING_ENV, tb.JEV_KEY_ENV)
                saved = {k: os.environ.pop(k, None) for k in keys}
                os.environ.update({tb.JEV_KEY_ENV: "jv-invented-key-5d1e", **env})
                try:
                    with self.assertRaises(Uploaded):
                        asyncio.run(agent.run("Do the task.", Env(), None))
                finally:
                    for k, v in saved.items():
                        os.environ.pop(k, None)
                        if v is not None:
                            os.environ[k] = v
                return tomllib.loads(got["text"])["kernel"]

        os.environ.setdefault("ANTHROPIC_API_KEY", "an-invented-key")
        for cls in (theseus_agent.Theseus, theseus_agent.TheseusRouted):
            k = kernel(cls, {})
            self.assertEqual(k["daily_spend_ceiling_usd"], 2 * k["spend_limit_usd"], cls)
            self.assertEqual(k["daily_spend_ceiling_mode"], "stop", cls)
            k = kernel(cls, {"THESEUS_BENCH_SPEND_LIMIT": "3", tb.DAY_CEILING_ENV: "9"})
            self.assertEqual((k["spend_limit_usd"], k["daily_spend_ceiling_usd"]), (3.0, 9.0), cls)

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
        self.assertEqual(asyncio.run(limit(theseus_agent.Theseus)), 3.75)
        self.assertEqual(asyncio.run(limit(theseus_agent.TheseusRouted)), 20.0)
        self.assertEqual(asyncio.run(limit(theseus_agent.TheseusRouted, "7.5")), 7.5)
        self.assertEqual(asyncio.run(limit(theseus_agent.Theseus, "3")), 3.0)
        # The reserve at 128,000 output tokens, input aside, per model.
        self.assertGreater(128_000 * 20.0 / 1e6, 2.0)   # Opus 5.5
        self.assertGreater(128_000 * 50.0 / 1e6, 2.0)   # Fable 5.1
        self.assertLess(128_000 * 50.0 / 1e6 + 10.0, tb.ROUTED_SPEND_LIMIT_USD)  # a 1M-token prompt


class NoCap(unittest.TestCase):
    """The gauntlet's no-cap switch (theseus-710ch, the owner 2026-10-25
    23:17: the $2 per-trial cap "comes off for every harness").

    It has to cover both halves of a cap that was never one thing: the two
    arms whose cap the harness really enforced (Theseus's `spend_limit_usd`
    and Claude Code's `--max-budget-usd`), and the six whose `max_budget_usd`
    was only recorded and used to flag a trial that passed it."""

    CAPPED: dict[str, str] = {}
    OFF = {msr.NO_CAP_ENV: "1"}

    def test_the_switch_reads_a_shells_truth_values(self):
        for v in ("1", "yes", "true", "on", "ON"):
            self.assertTrue(msr.no_cap({msr.NO_CAP_ENV: v}), v)
        for v in ("", " ", "0", "off", "false", "no", "NO"):
            self.assertFalse(msr.no_cap({msr.NO_CAP_ENV: v}), repr(v))
        self.assertFalse(msr.no_cap(self.CAPPED))

    def test_a_recorded_cap_is_dropped_and_an_enforced_one_is_not_asked_for(self):
        """`budget_cap` is the one place every arm asks: None under the
        switch, so a harness that enforces a cap is given none and a harness
        that only records one flags nothing. The turn cap is not a spend cap
        and never passes through here."""
        self.assertEqual(msr.budget_cap(2.0, self.CAPPED), 2.0)
        self.assertEqual(msr.budget_cap(5.0, self.CAPPED), 5.0)
        self.assertIsNone(msr.budget_cap(2.0, self.OFF))
        self.assertIsNone(msr.budget_cap(None, self.CAPPED))
        self.assertIsNone(msr.budget_cap(None, self.OFF))

    def test_the_theseus_arms_limit_becomes_a_runaway_guard_not_no_limit(self):
        """A trial that loops must still be refused, because a driver's spend
        stop counts finished trials only and overshoots by about one trial
        times the parallelism (memory `tb-driver-spend-stop-latches`). The
        guard is far above anything any arm has run: the most expensive
        uncapped trial on record is Codex's $6.54 (10-25)."""
        self.assertEqual(tb.trial_budget_usd(self.OFF), tb.NO_CAP_TRIAL_LIMIT_USD)
        self.assertGreater(tb.NO_CAP_TRIAL_LIMIT_USD, 7 * 6.54)
        plain = tb.spend_limit_usd("claude-sonnet-5-5", self.OFF)
        self.assertEqual(plain, 51.75)
        self.assertGreater(plain, tb.spend_limit_usd("claude-sonnet-5-5", self.CAPPED))
        # The routed arm's $20 was the budget plus Fable's reserve; so is this.
        self.assertEqual(tb.routed_spend_limit_usd(self.CAPPED), tb.ROUTED_SPEND_LIMIT_USD)
        self.assertEqual(tb.routed_spend_limit_usd(self.OFF),
                         round(tb.NO_CAP_TRIAL_LIMIT_USD + tb.reservation_usd("claude-fable-5-1"), 2))

    def test_the_guard_still_clears_one_reservation_at_every_model(self):
        """The reservation must fit under the limit or the kernel refuses the
        call and the trial ends at exit 5 having spent almost nothing —
        `theseus-xkzsn`'s hazard, which would read as a weak arm rather than
        as a broken cell. So the guard replaces the budget term only."""
        for m in ("claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1", "claude-haiku-5-5"):
            limit = tb.spend_limit_usd(m, self.OFF)
            self.assertGreaterEqual(limit, tb.NO_CAP_TRIAL_LIMIT_USD + tb.reservation_usd(m), m)
            # And a trial at the guard's edge is not refused by the reserve alone.
            self.assertGreaterEqual(limit, tb.NO_CAP_TRIAL_LIMIT_USD - 0.01 + tb.reservation_usd(m), m)

    def test_the_profile_the_trial_writes_carries_the_guard_and_its_day_ceiling(self):
        values = tb.settings("claude-sonnet-5-5", "/app",
                             spend_limit_usd=tb.spend_limit_usd("claude-sonnet-5-5", self.OFF))
        k = {key: v for (table, key), v in values.items() if table == "kernel"}
        self.assertEqual(k["spend_limit_usd"], 51.75)
        # The ceiling stays above the limit, and still stops rather than warns.
        self.assertGreater(k["daily_spend_ceiling_usd"], k["spend_limit_usd"])
        self.assertEqual(k["daily_spend_ceiling_mode"], tb.DAY_CEILING_MODE)

    def test_a_trials_record_says_it_ran_uncapped_and_whether_the_old_cap_would_have_cut_it(self):
        """The page has to put an uncapped cell beside a capped one, so the
        record carries the cap that was dropped as well as the fact of it."""
        rec = {"list_cost_usd": 3.5, "model_calls": 9}
        off = ef.limits(rec, msr.budget_cap(2.0, self.OFF), 200, False, cap_asked_usd=2.0)
        self.assertEqual((off["no_cap"], off["max_budget_usd"], off["cap_asked_usd"]), (True, None, 2.0))
        self.assertIs(off["over_budget"], None)          # no cap: nothing to pass
        self.assertIs(off["would_have_been_capped"], True)
        self.assertIs(off["over_turns"], False)          # the turn cap is untouched
        on = ef.limits(rec, msr.budget_cap(2.0, self.CAPPED), 200, True, cap_asked_usd=2.0)
        self.assertEqual((on["no_cap"], on["max_budget_usd"]), (False, 2.0))
        self.assertIs(on["over_budget"], True)
        # A cheap uncapped trial the old cap would not have touched.
        cheap = ef.limits({"list_cost_usd": 0.12, "model_calls": 9}, None, 200, False, cap_asked_usd=2.0)
        self.assertIs(cheap["would_have_been_capped"], False)
        # No cap asked for at all: the fields say so rather than guessing.
        none = ef.limits(rec, None, 200, False)
        self.assertEqual((none["no_cap"], none["cap_asked_usd"], none["would_have_been_capped"]),
                         (False, None, None))

    def test_pis_record_carries_the_same_three_fields(self):
        """Pi's limits are built by their own function, so they can drift."""
        spend = {"cost_usd": 3.5}
        off = ef.pi_limits(spend, 4, msr.budget_cap(2.0, self.OFF), 200, 3, cap_asked_usd=2.0)
        self.assertEqual((off["no_cap"], off["cap_asked_usd"], off["would_have_been_capped"]),
                         (True, 2.0, True))
        self.assertIs(off["over_budget"], None)
        self.assertFalse(off["enforced"])

    def test_run_py_takes_the_cap_off_by_default_and_records_which_it_took(self):
        """The owner's decision is the default, and `--cap` reproduces a
        capped cell. Either way the state is in the run's record, so the two
        are never read as one series."""
        import run as runmod
        with tempfile.TemporaryDirectory() as d:
            plan = Path(d) / "p.json"
            plan.write_text(json.dumps({"name": "p", "k": 1}))
            lines: list[list[str]] = []
            for argv, want in (([], True), (["--no-cap"], True), (["--cap"], False)):
                before = os.environ.get(msr.NO_CAP_ENV)
                try:
                    with unittest.mock.patch.object(
                            runmod, "prepare",
                            side_effect=lambda *_a, **_k: {"job_name": "j", "jobs_dir": d,
                                                           "plan": {}, "host": {}, "parallel": 4,
                                                           "lines": [["harbor", "run"]]}):
                        code = runmod.main(["--plan", str(plan), *argv, "--", "harbor", "run"],
                                           runner=lambda line: lines.append(line) or 0)
                    self.assertEqual(code, 0, argv)
                    self.assertIs(msr.no_cap(), want, argv)
                    rec = json.loads((Path(d) / "j.run.json").read_text())
                    self.assertIs(rec["no_cap"], want, argv)
                finally:
                    if before is None:
                        os.environ.pop(msr.NO_CAP_ENV, None)
                    else:
                        os.environ[msr.NO_CAP_ENV] = before
            # `--no-cap --cap` is a contradiction, not a silent last-wins.
            with self.assertRaises(SystemExit):
                runmod.main(["--plan", str(plan), "--no-cap", "--cap", "--", "harbor", "run"])


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
    echo "$*" > "$STANDIN_DIR/ask-args"
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
              routed: bool = False, follow_for: str = tb.FOLLOW_FOR,
              deadline: int | None = None) -> subprocess.Popen:
        env = dict(os.environ, STANDIN_DIR=self.tmp.name, STANDIN_ASK=ask,
                   THESEUS_BENCH_INSTRUCTION="Fix the repository's history.")
        if path is not None:
            env["PATH"] = path
        script = tb.run_script(str(self.bin), str(self.state), str(self.logs),
                               str(SAMPLER) if sampler else None, 50, routed, follow_for, deadline)
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

    def test_the_ask_follows_nothing_unless_a_bound_is_given(self):
        """theseus-mqxk's review, before the bench rerun: `ask` under `--spawn`
        follows its later work for 30 minutes by default; a trial asks for 0,
        so it ends with its turn as the runs it compares with did, unless the
        run names a bound (THESEUS_BENCH_FOLLOW_FOR)."""
        args = Path(self.tmp.name) / "ask-args"
        p = self.start("done")
        _, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertEqual(tb.FOLLOW_FOR, "0")
        self.assertIn(" ask --follow-for 0 -", args.read_text())
        p = self.start("done", follow_for="25m")
        p.communicate(timeout=30)
        self.assertIn(" ask --follow-for 25m -", args.read_text())

    def test_the_ask_carries_the_deadline_it_is_given(self):
        """theseus-5mkz1: a deadline reaches `ask --deadline`, so the turn is
        told its time; without one the ask has no such flag."""
        args = Path(self.tmp.name) / "ask-args"
        p = self.start("done", deadline=3540)
        _, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertIn(" ask --follow-for 0 --deadline 3540 -", args.read_text())
        p = self.start("done")
        p.communicate(timeout=30)
        self.assertNotIn("--deadline", args.read_text())

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

    def test_a_followed_runs_turns_are_all_answered(self):
        """A followed run's result (theseus-mqxk) sums its turns: the ask's
        answers are no unanswered spend of the last turn's."""
        h = json.loads(json.dumps(HISTORY))
        for n in h["nodes"]:
            n["turn_id"] = "turn_1"
        h["nodes"][-1]["turn_id"] = "turn_2"
        turn = {"turn_id": "turn_2", "stop_reason": "no_tool_calls", "cost_usd": 0.03,
                "usage": {"input_tokens": 150, "output_tokens": 60,
                          "cache_read_input_tokens": 2500, "cache_creation_input_tokens": 500},
                "asked": {"turn_id": "turn_1"}, "continuations": [{"turn_id": "turn_2"}]}
        t = atif.trajectory(h, version="0.0.1", turn=turn)
        self.assertEqual(len(t["steps"]), 3)
        self.assertEqual(t["final_metrics"]["total_cost_usd"], 0.03)

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

    def test_its_install_adds_claude_codes_packages_before_the_agents_clock(self):
        """Harbor's Claude Code installer adds curl, bash, nodejs, npm and
        procps in `install`, which Harbor runs before the agent's timed
        phase. The Theseus arm's install runs the same install first (before
        its upload of the binaries), and its `run` installs nothing."""
        import asyncio
        import theseus_agent
        from harbor.agents.installed.claude_code import ClaudeCode

        # The same set as Harbor's own installer asks for.
        import inspect
        self.assertIn('("curl", "bash", "nodejs", "npm", "procps")', inspect.getsource(ClaudeCode.install))
        self.assertEqual(theseus_agent.SYSTEM_PACKAGES, ("curl", "bash", "nodejs", "npm", "procps"))

        events = []

        class Env:
            default_user = None

            async def exec(self, command, **kw):
                events.append(("exec", command, kw.get("user")))
                rc = 1 if command.startswith("command -v") else 0
                out = "apt-get" if "for manager in" in command else ""
                return type("R", (), {"stdout": out, "stderr": "", "return_code": rc})()

            async def upload_file(self, source, target):
                events.append(("upload", str(target), None))

        with tempfile.TemporaryDirectory() as d, tempfile.TemporaryDirectory() as bins:
            for b in ("theseus", "theseusd"):
                (Path(bins) / b).write_text("x")
            os.environ["THESEUS_BENCH_BIN_DIR"] = bins
            agent = theseus_agent.Theseus(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
            asyncio.run(agent.install(Env()))
        installs = [i for i, e in enumerate(events) if e[0] == "exec" and "apt-get install" in e[1]]
        self.assertEqual(len(installs), 1, events)
        cmd = events[installs[0]][1]
        for pkg in ("curl", "bash", "nodejs", "npm", "procps"):
            self.assertIn(pkg, cmd.split())
        self.assertEqual(events[installs[0]][2], "root")
        first_upload = min(i for i, e in enumerate(events) if e[0] == "upload")
        self.assertLess(installs[0], first_upload, "the packages go in before the binaries")
        import inspect as _i
        self.assertNotIn("ensure_system_dependencies", _i.getsource(theseus_agent.Theseus.run))

    def test_two_builds_tell_apart_in_the_record_by_their_commit(self):
        """`theseus --version` says 0.0.1 for every build, so the install writes the commit bench/build.sh left
        beside the binaries to the trial's logs, and the record names it beside the version (theseus-p3jl)."""
        import asyncio
        import theseus_agent
        from harbor.models.agent.context import AgentContext

        commands = []

        class Env:
            default_user = None

            async def exec(self, command, **kw):
                commands.append(command)
                return type("R", (), {"stdout": "", "stderr": "", "return_code": 0})()

            async def upload_file(self, source, target):
                pass

        recs = []
        with tempfile.TemporaryDirectory() as d:
            for commit in ("1111111111111111111111111111111111111111", "2222222222222222222222222222222222222222-dirty"):
                bins, logs = Path(d) / f"bin-{commit[:4]}", Path(d) / f"logs-{commit[:4]}"
                bins.mkdir()
                logs.mkdir()
                for b in ("theseus", "theseusd"):
                    (bins / b).write_text("x")
                (bins / "build-commit").write_text(commit + "\n")
                os.environ["THESEUS_BENCH_BIN_DIR"] = str(bins)
                commands.clear()
                agent = theseus_agent.Theseus(logs_dir=logs, model_name="anthropic/claude-sonnet-5-5",
                                              environment_logs_dir=logs)
                asyncio.run(agent.install(Env()))
                wrote = [c for c in commands if "build-commit.txt" in c]
                self.assertEqual(wrote, [theseus_agent.measure.build_script(commit, logs.as_posix())])
                subprocess.run(["sh", "-c", wrote[0]], check=True)
                (logs / "version.txt").write_text("theseus 0.0.1\n")
                agent.populate_context_post_run(AgentContext())
                recs.append(json.loads((logs / "efficiency.json").read_text()))
            self.assertEqual([r["version"] for r in recs], ["theseus 0.0.1"] * 2, "the version cannot tell them apart")
            self.assertEqual([r["build_commit"] for r in recs],
                             ["1111111111111111111111111111111111111111",
                              "2222222222222222222222222222222222222222-dirty"])
            # A build with no commit file records none, and the install says nothing.
            (bins / "build-commit").unlink()
            commands.clear()
            asyncio.run(agent.install(Env()))
            self.assertEqual([c for c in commands if "build-commit.txt" in c], [])
        self.assertIn("build-commit", (Path(__file__).resolve().parent.parent / "build.sh").read_text())

    def test_it_loads_as_an_atif_agent_with_an_error_for_each_end(self):
        import theseus_agent

        self.assertEqual(theseus_agent.Theseus.name(), "theseus")
        self.assertTrue(theseus_agent.Theseus.capabilities.atif)
        ends = {code for code, end in tb.ENDS.items() if end not in ("done", "failed", "usage", "unreachable")}
        self.assertEqual(set(theseus_agent.ERRORS), ends)
        self.assertEqual(theseus_agent.SAMPLER, "/installed-agent/bin/sampler.py")


class ListPrices(unittest.TestCase):
    """The price rows a non-Anthropic arm needs (theseus-01n2l)."""

    NEW = ("glm-5.3", "kimi-k3.1-pro", "deepseek-v4-pro", "claude-sonnet-5", "claude-opus-5")

    def test_every_row_has_the_four_classes_positive(self):
        for name, row in ef.LIST_PRICES.items():
            self.assertEqual(set(row), set(ef.CLASSES), name)
            for c in ef.CLASSES:
                self.assertGreater(row[c], 0, f"{name} {c}")

    def test_a_reservation_prices_each_new_model(self):
        for m in self.NEW:
            for name in (m, f"someprovider/{m}"):
                r = tb.reservation_usd(name)
                self.assertIsInstance(r, float, name)
                self.assertGreater(r, 0, name)
            p = ef.LIST_PRICES[m]
            cap = ef.MAX_OUTPUT_TOKENS.get(m, tb.RESERVE_OUTPUT_TOKENS)
            worst = (cap * p["output"] + tb.RESERVE_INPUT_TOKENS * p["input"]) / 1e6
            self.assertGreaterEqual(tb.reservation_usd(m), worst, m)

    def test_a_limit_leaves_room_for_one_reservation_at_the_models_own_output_cap(self):
        """theseus-xkzsn: the kernel reserves the catalog's `max_output_tokens`
        at the output price, not a flat 128,000. DeepSeek's is 384,000, so one
        call reserves about $1.52 of output alone; a trial that has spent just
        under the $2 budget must still be allowed that call."""
        capped: dict[str, str] = {}
        caps = {"deepseek-v4-pro": 384_000, "kimi-k3.1-pro": 131_072, "glm-5.3": 128_000}
        for m, cap in caps.items():
            p = ef.LIST_PRICES[m]
            need = (cap * p["output"] + 128_000 * p["input"]) / 1e6
            limit = tb.spend_limit_usd(m, capped)
            self.assertGreaterEqual(limit, 1.99 + need, f"{m}: limit {limit} < 1.99 + reservation {need:.4f}")

    def test_every_priced_model_has_an_output_cap_row_and_no_stray_ones(self):
        self.assertEqual(set(ef.MAX_OUTPUT_TOKENS) - set(ef.LIST_PRICES), set())
        for m in ef.LIST_PRICES:
            if m.startswith("gpt-"):
                continue   # not in the catalog: the 128,000 default
            self.assertIn(m, ef.MAX_OUTPUT_TOKENS, m)

    def test_the_output_caps_match_the_rust_catalog(self):
        """The mirror is compared with `catalog.rs`'s `builtin()` rows by
        source text, so a divergence fails here rather than in a run."""
        src = (Path(__file__).resolve().parents[2] / "crates/theseus-core/src/catalog.rs")
        if not src.exists():
            self.skipTest("catalog.rs not in this tree")
        text = src.read_text()
        helper = {"deepseek": 384_000, "kimi": 131_072}   # the helper fns' literals
        for fn, want in helper.items():
            m = re.search(rf"fn {fn}\(.*?max_output_tokens: ([\d_]+),", text, re.S)
            self.assertIsNotNone(m, fn)
            self.assertEqual(int(m.group(1).replace("_", "")), want, fn)
        rows = {"deepseek-v4-pro": r'"deepseek-v4-pro"\.into\(\),\s*deepseek\(',
                "kimi-k3.1-pro": r'"kimi-k3\.1-pro"\.into\(\),\s*kimi\(',
                "glm-5.3": r'"glm-5\.3"\.into\(\),.*?glm\(([\d_]+), ([\d_]+),'}
        for model, pat in rows.items():
            m = re.search(pat, text, re.S)
            self.assertIsNotNone(m, model)
            got = int(m.group(2).replace("_", "")) if model == "glm-5.3" else helper[model.split("-")[0]]
            self.assertEqual(ef.MAX_OUTPUT_TOKENS[model], got, model)
        for model in ("claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1", "claude-haiku-5-5",
                      "claude-sonnet-5", "claude-opus-5"):
            m = re.search(rf'"{model}"\.into\(\),.*?claude\(\s*m, ([\d_]+),', text, re.S)
            self.assertIsNotNone(m, model)
            self.assertEqual(ef.MAX_OUTPUT_TOKENS[model], int(m.group(1).replace("_", "")), model)

    def test_one_dataset_of_a_cn_model_and_a_claude_model_prices_completely(self):
        by = {
            "claude-sonnet-5": {"input": 1_000_000, "cache_read": 2_000_000, "cache_write": 0, "output": 500_000},
            "zai/glm-5.3": {"input": 3_000_000, "cache_read": 1_000_000, "cache_write": 0, "output": 1_000_000},
        }
        s, g = ef.LIST_PRICES["claude-sonnet-5"], ef.LIST_PRICES["glm-5.3"]
        want = (1 * s["input"] + 2 * s["cache_read"] + 0.5 * s["output"]
                + 3 * g["input"] + 1 * g["cache_read"] + 1 * g["output"])
        got = ef.list_price(by)
        self.assertIsNotNone(got)
        self.assertAlmostEqual(got, want)
        # The two Opus models are one character apart and priced apart.
        self.assertNotEqual(ef.LIST_PRICES["claude-opus-5"], ef.LIST_PRICES["claude-opus-5-5"])


class Providers(unittest.TestCase):
    """A provider carried from `-m` into the profile (theseus-fhwo7). The
    providers here are stand-ins: no real key, and no real provider's
    variable, is named in a test."""

    FAKE = {"fakeprov": {"api_base": "https://fake.invalid/anthropic",
                         "secret": "fakeprov_api_key", "key_env": "FAKEPROV_PLACEHOLDER_VAR"}}

    def render(self, name):
        provider, model = tb.split_model(name)
        values = tb.settings(model, "/app", spend_limit_usd=3.75, day_ceiling_usd=7.5,
                             provider=provider)
        return tb.profile(tb.PROFILE.read_text(), values)

    def test_a_fake_provider_lands_in_the_profile(self):
        with unittest.mock.patch.dict(tb.PROVIDERS, self.FAKE):
            self.assertEqual(tb.split_model("fakeprov/fake-model-1"), ("fakeprov", "fake-model-1"))
            cfg = tomllib.loads(self.render("fakeprov/fake-model-1"))
            self.assertEqual(tb.key_env("fakeprov"), "FAKEPROV_PLACEHOLDER_VAR")
        self.assertEqual(cfg["model"]["provider"], "fakeprov")
        self.assertEqual(cfg["model"]["model"], "fake-model-1")
        self.assertEqual(cfg["profiles"]["bench"]["provider"], "fakeprov")
        self.assertEqual(cfg["profiles"]["bench"]["model"], "fake-model-1")
        block = cfg["providers"]["fakeprov"]
        self.assertEqual(block["api_base"], "https://fake.invalid/anthropic")
        self.assertEqual(block["api_key_secret"], "fakeprov_api_key")
        self.assertEqual(block["kind"], "anthropic_messages")
        self.assertEqual(cfg["model"]["api_key_secret"], "fakeprov_api_key")
        self.assertEqual(cfg["secrets"]["fakeprov_api_key"], "env:FAKEPROV_PLACEHOLDER_VAR")

    def test_an_unknown_provider_raises_naming_the_supported(self):
        with self.assertRaises(ValueError) as ctx:
            tb.split_model("zia/glm-5.3")
        for known in ("anthropic", *tb.PROVIDERS):
            self.assertIn(known, str(ctx.exception))

    def test_the_catalogs_provider_spellings(self):
        self.assertEqual(set(tb.PROVIDERS), {"zai", "moonshot", "deepseek"})
        for p in tb.PROVIDERS.values():
            self.assertTrue(p["api_base"].startswith("https://"))
            self.assertRegex(p["key_env"], r"^[A-Z][A-Z0-9_]*_API_KEY$")

    def test_the_anthropic_profile_is_byte_unchanged(self):
        # Hashed from the profile as it rendered before the provider plumbing
        # (a regression guard: it passed on the base, as it should).
        import hashlib

        want = "189d8e65a1675e7bcc5c205fa497b61bca4ab8952b601bc2cddc5c67ca40376b"
        for name in ("anthropic/claude-sonnet-5-5", "claude-sonnet-5-5", None):
            self.assertEqual(hashlib.sha256(self.render(name).encode()).hexdigest(), want, name)
        self.assertNotIn("[providers", self.render(None))

    def test_the_routed_arm_refuses_another_provider(self):
        with unittest.mock.patch.dict(tb.PROVIDERS, self.FAKE):
            with self.assertRaises(ValueError):
                tb.settings("fake-model-1", "/app", provider="fakeprov", routed=True)

    def test_no_key_material_in_a_rendered_profile(self):
        # The hole stays a hole: only variable names, never a value.
        env = {k: "not-a-key-sentinel" for k in
               ("ANTHROPIC_API_KEY", *(p["key_env"] for p in tb.PROVIDERS.values()))}
        with unittest.mock.patch.dict(os.environ, env), \
                unittest.mock.patch.dict(tb.PROVIDERS, self.FAKE):
            for name in ("anthropic/claude-sonnet-5-5", "fakeprov/fake-model-1", "zai/glm-5.3"):
                text = self.render(name)
                self.assertNotIn("sentinel", text, name)
                cfg = tomllib.loads(text)
                for ref in cfg["secrets"].values():
                    self.assertRegex(ref, r"^env:[A-Z][A-Z0-9_]*$", name)


if __name__ == "__main__":
    unittest.main()

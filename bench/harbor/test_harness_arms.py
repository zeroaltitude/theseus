"""Tests for the arms built on Harbor's own agents (theseus-qags): Codex CLI, Aider,
OpenCode, OpenHands and OpenClaw.

    python3 -m unittest discover -s bench/harbor                 # the records
    .venv/bin/python -m unittest discover -s bench/harbor         # with Harbor: the agents too

The records are read from fixtures in each harness's own format (the content invented, the
numbers worked by hand). The agents' checks need Harbor and skip on the standard library.
"""

from __future__ import annotations

import asyncio
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import efficiency as ef

try:
    from harbor.agents.installed.aider import Aider
    from harbor.agents.installed.codex import Codex
    from harbor.agents.installed.opencode import OpenCode
    from harbor.agents.installed.openclaw import OpenClaw
    from harbor.agents.installed.openhands_sdk import OpenHandsSDK as OpenHands
    from harbor.environments.base import ExecResult
    from harbor.models.agent.context import AgentContext

    import aider_agent
    import codex_agent
    import measured
    import opencode_agent
    import openclaw_agent
    import openhands_agent
except ImportError:  # Harbor is not installed: these skip.
    Codex = None

SONNET = "anthropic/claude-sonnet-5-5"

# OpenCode's `run --format=json` stream: two model calls, one tool call, a reasoning count.
OPENCODE = "\n".join(json.dumps(e) for e in [
    {"type": "step_start", "timestamp": 1_000_000, "sessionID": "ses_1", "part": {"type": "step-start"}},
    {"type": "tool_use", "timestamp": 1_002_000, "part": {"type": "tool", "tool": "bash", "callID": "c1"}},
    {"type": "step_finish", "timestamp": 1_003_000, "part": {
        "type": "step-finish", "cost": 0.0105,
        "tokens": {"input": 10, "output": 400, "reasoning": 100, "cache": {"read": 0, "write": 2000}}}},
    {"type": "step_start", "timestamp": 1_004_000, "part": {"type": "step-start"}},
    {"type": "text", "timestamp": 1_005_000, "part": {"type": "text", "text": "Done."}},
    {"type": "step_finish", "timestamp": 1_006_500, "part": {
        "type": "step-finish", "cost": 0.0034,
        "tokens": {"input": 5, "output": 200, "reasoning": 0, "cache": {"read": 2000, "write": 300}}}},
]) + "\n"

# Aider's output: two model calls' token lines, and a shell command it ran.
AIDER = """Aider v0.86.2
Main model: anthropic/claude-sonnet-5-5 with diff edit format
Tokens: 4.2k sent, 4.0k cache write, 310 received. Cost: $0.0132 message, $0.0132 session.
Running nginx -t
Tokens: 5.1k sent, 1.0k cache write, 4.0k cache hit, 1.2k received.
Cost: $0.0170 message, $0.0302 session.
"""
AIDER_ANALYTICS = [
    {"event": "launched", "properties": {}},
    {"event": "message_send", "properties": {"prompt_tokens": 4213, "completion_tokens": 312, "cost": 0.01321}},
    {"event": "message_send", "properties": {"prompt_tokens": 5108, "completion_tokens": 1187, "cost": 0.01699}},
]


class Records(unittest.TestCase):
    def test_every_new_arm_has_its_harness_names(self):
        for arm in ("codex", "aider", "opencode", "openhands", "openclaw"):
            self.assertTrue(ef.ARMS[arm]["names"], arm)
            self.assertEqual(ef.ARMS[arm]["wrapper_args"], ())

    def test_list_price_matches_the_bills_of_b5(self):
        """A b5 Theseus trial: 16 input, 101,895 cache reads, 22,896 cache writes, 19,149
        output; its own bill was $0.269141."""
        by = {"claude-sonnet-5-5": {"input": 16, "cache_read": 101895, "cache_write": 22896, "output": 19149}}
        self.assertAlmostEqual(ef.list_price(by), 0.269141, places=6)
        self.assertAlmostEqual(ef.list_price({"anthropic/claude-sonnet-5-5": by["claude-sonnet-5-5"]}),
                               0.269141, places=6)
        self.assertIsNone(ef.list_price({"some-model": {"input": 1}}))
        self.assertIsNone(ef.list_price({}))
        # A model with no tokens does not make the price unknown.
        self.assertAlmostEqual(ef.list_price({**by, "some-model": {"input": 0}}), 0.269141, places=6)

    def test_limits_flag_a_trial_past_the_caps_it_was_not_given(self):
        rec = {"cost_usd": 1.0, "list_cost_usd": 2.5, "model_calls": 201}
        lim = ef.limits(rec, 2.0, 200, False)
        self.assertEqual((lim["enforced"], lim["over_budget"], lim["over_turns"], lim["turns"]),
                         (False, True, True, 201))
        # The harness's own dollars when there is no list price.
        lim = ef.limits({"cost_usd": 1.0, "model_calls": 3}, 2.0, 200, True)
        self.assertEqual((lim["enforced"], lim["over_budget"], lim["over_turns"]), (True, False, False))
        self.assertIsNone(ef.limits({}, None, None, False)["over_budget"])

    def test_opencode_counts_each_step_finish_as_a_call(self):
        sp = ef.opencode_spend(ef.read_jsonl_text(OPENCODE))
        self.assertEqual((sp["model_calls"], sp["tool_calls"], sp["spend_from"]), (2, 1, "stream"))
        # Reasoning is billed as output; the cache's reads and writes kept apart.
        self.assertEqual(sp["tokens"], {"input": 15, "cache_read": 2000, "cache_write": 2300, "output": 700})
        self.assertAlmostEqual(sp["cost_usd"], 0.0139)
        self.assertEqual(ef.opencode_wall(ef.read_jsonl_text(OPENCODE)), 6.5)

    def test_opencode_unpriced_call_leaves_the_total_unknown(self):
        events = ef.read_jsonl_text(OPENCODE)
        del events[2]["part"]["cost"]
        self.assertIsNone(ef.opencode_spend(events)["cost_usd"])

    def test_aider_reads_its_token_lines_and_the_exact_counts(self):
        calls = ef.aider_calls(AIDER)
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0], {"input": 200, "cache_read": 0, "cache_write": 4000, "output": 310,
                                    "cost_usd": 0.0132})
        self.assertEqual(calls[1], {"input": 100, "cache_read": 4000, "cache_write": 1000, "output": 1200,
                                    "cost_usd": 0.0170})
        sp = ef.aider_spend(AIDER, AIDER_ANALYTICS)
        # The analytics log's exact counts replace the rounded ones; the split stays the line's.
        self.assertEqual(sp["spend_from"], "analytics")
        self.assertEqual(sp["tokens"], {"input": 213 + 108, "cache_read": 4000, "cache_write": 5000,
                                        "output": 312 + 1187})
        self.assertAlmostEqual(sp["cost_usd"], 0.0302)
        self.assertEqual((sp["model_calls"], sp["tool_calls"]), (2, 1))
        # Without the log, the lines alone.
        self.assertEqual(ef.aider_spend(AIDER, [])["spend_from"], "stream")
        self.assertEqual(ef.aider_spend("", [])["spend_from"], None)

    def test_openhands_reads_each_call_from_the_wrappers_metrics(self):
        metrics = {"llms": [{"usage_id": "agent", "model": "anthropic/claude-sonnet-5-5", "metrics": {
            "accumulated_cost": 0.0157,
            "token_usages": [{"prompt_tokens": 6019, "completion_tokens": 4, "cache_read_tokens": 0,
                              "cache_write_tokens": 6008},
                             {"prompt_tokens": 6019, "completion_tokens": 4, "cache_read_tokens": 6008,
                              "cache_write_tokens": 0}]}}]}
        traj = {"steps": [{"source": "agent", "tool_calls": [{"tool_call_id": "a"}]},
                          {"source": "agent"}], "final_metrics": {"total_prompt_tokens": 12038}}
        sp = ef.openhands_spend(metrics, traj)
        self.assertEqual((sp["spend_from"], sp["model_calls"], sp["tool_calls"], sp["cost_usd"]),
                         ("metrics", 2, 1, 0.0157))
        self.assertEqual(sp["tokens"], {"input": 22, "cache_read": 6008, "cache_write": 6008, "output": 8})
        self.assertIn("claude-sonnet-5-5", sp["by_model"])
        # Without the wrapper's file, Harbor's totals (the write lost in them).
        sp = ef.openhands_spend(None, traj)
        self.assertEqual((sp["spend_from"], sp["model_calls"]), ("trajectory_totals", 2))

    def test_records_from_a_trials_files(self):
        with tempfile.TemporaryDirectory() as d:
            logs = Path(d)
            (logs / "opencode.txt").write_text(OPENCODE)
            rec = ef.opencode_record(logs)
            self.assertEqual((rec["arm"], rec["model_calls"]), ("opencode", 2))
            # 15 x 2 + 2000 x 0.2 + 2300 x 2.5 + 700 x 10, a million.
            self.assertAlmostEqual(rec["list_cost_usd"], 0.01318, places=6)
            self.assertEqual(rec["sampler"]["status"], "missing")
            (logs / "aider.txt").write_text(AIDER)
            (logs / "aider-analytics.jsonl").write_text("\n".join(json.dumps(e) for e in AIDER_ANALYTICS))
            self.assertEqual(ef.aider_record(logs)["arm"], "aider")
            traj = {"steps": [
                {"source": "user", "timestamp": "2026-10-07T12:00:00Z"},
                {"source": "agent", "timestamp": "2026-10-07T12:00:04Z", "model_name": "gpt-6.1-sol",
                 "tool_calls": [{"id": "a"}], "metrics": {"prompt_tokens": 1000, "cached_tokens": 600,
                                                          "completion_tokens": 50, "cost_usd": 0.0014}}],
                "final_metrics": {"total_cost_usd": 0.0014}}
            (logs / "trajectory.json").write_text(json.dumps(traj))
            rec = ef.codex_record(logs)
            self.assertEqual((rec["arm"], rec["model_calls"], rec["tool_calls"], rec["cost_usd"]),
                             ("codex", 1, 1, 0.0014))
            self.assertEqual(rec["tokens"], {"input": 400, "cache_read": 600, "cache_write": 0, "output": 50})
            self.assertEqual(rec["wall_s"], 4.0)
            # 400 x 2 + 600 x 0.1 + 50 x 10, a million.
            self.assertAlmostEqual(rec["list_cost_usd"], 0.00136, places=6)
            # Codex's cache writes, as Harbor's converter names them, are writes, not input.
            traj["steps"][1]["metrics"]["prompt_tokens"] = 1300
            traj["steps"][1]["metrics"]["extra"] = {"cache_write_input_tokens": 300}
            (logs / "trajectory.json").write_text(json.dumps(traj))
            self.assertEqual(ef.codex_record(logs)["tokens"],
                             {"input": 400, "cache_read": 600, "cache_write": 300, "output": 50})


class FakeEnvironment:
    default_user = "agent"

    def __init__(self):
        self.commands: list[str] = []
        self.users: list[str | None] = []
        self.envs: list[dict | None] = []
        self.uploads: list[tuple[str, str]] = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        self.commands.append(command)
        self.users.append(user)
        self.envs.append(env)
        return ExecResult(stdout="", stderr="", return_code=0)

    async def upload_file(self, source_path, target_path):
        self.uploads.append((str(source_path), target_path))


def arms():
    return [
        (codex_agent.MeasuredCodex, Codex, codex_agent.NATIVE_MODEL, "codex", "0.161.0", "reasoning_effort"),
        (aider_agent.MeasuredAider, Aider, SONNET, "aider", "0.86.2", None),
        (opencode_agent.MeasuredOpenCode, OpenCode, SONNET, "opencode", "1.18.35", "variant"),
        (openhands_agent.MeasuredOpenHands, OpenHands, SONNET, "openhands", "1.53.0", "reasoning_effort"),
        (openclaw_agent.MeasuredOpenClaw, OpenClaw, SONNET, "openclaw", "2026.9.8", "thinking"),
    ]


@unittest.skipIf(Codex is None, "Harbor is not installed")
class Arms(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def make(self, cls, model, **kw):
        return cls(logs_dir=self.logs, model_name=model, max_budget_usd=2.0, max_turns=200, **kw)

    def test_each_is_harbors_own_by_name_pinned_at_medium(self):
        for cls, base, model, arm, pin, effort in arms():
            with self.subTest(arm):
                a = self.make(cls, model)
                self.assertTrue(issubclass(cls, base))
                self.assertEqual(cls.name(), base.name())
                self.assertEqual((cls.ARM, a.version(), a.effort_asked()), (arm, pin, "medium"))
                if effort:
                    self.assertEqual(getattr(a.options, effort), "medium")
                self.assertEqual(a.caps, {"max_budget_usd": 2.0, "max_turns": 200})
                # An --ak wins over the pin and over medium: an ablation.
                b = self.make(cls, model, version="9.9.9", **{effort or "reasoning_effort": "high"})
                self.assertEqual((b.version(), b.effort_asked()), ("9.9.9", "high"))

    def test_harbor_takes_the_caps_as_options_before_it_builds_the_agent(self):
        """Harbor checks every --ak against the options model first (its
        `harbor run` refused `max_budget_usd` for OpenCode's own model)."""
        for cls, base, model, arm, _, effort in arms():
            with self.subTest(arm):
                kw = {"max_budget_usd": "2.0", "max_turns": "200"}
                opts = cls.options_model.model_validate(kw)
                self.assertEqual((opts.max_budget_usd, opts.max_turns), (2.0, 200))
                with self.assertRaises(Exception):
                    base.options_model.model_validate(kw)
                self.assertNotIn("max_budget", self.make(cls, model).build_cli_flags())

    def test_no_new_arm_enforces_the_spend_cap_and_openhands_enforces_the_turns(self):
        for cls, _, model, arm, _, _ in arms():
            with self.subTest(arm):
                self.assertFalse(self.make(cls, model).caps_enforced())
        oh = self.make(openhands_agent.MeasuredOpenHands, SONNET)
        self.assertEqual(oh.options.max_iterations, 200)
        self.assertEqual(oh.options.reasoning_effort, "medium")
        # Caps given as the CLI's strings are read as numbers.
        s = openhands_agent.MeasuredOpenHands(logs_dir=self.logs, model_name=SONNET, max_budget_usd="2.0",
                                              max_turns="200")
        self.assertEqual(s.caps, {"max_budget_usd": 2.0, "max_turns": 200})

    def test_the_sampler_runs_around_harbors_run_and_a_timeout_stops_the_agent(self):
        for cls, base, model, arm, _, _ in arms():
            with self.subTest(arm):
                a = self.make(cls, model)

                async def ends(self_, instruction, environment, context):
                    environment.commands.append("harbor's run")

                env = FakeEnvironment()
                with mock.patch.object(base, "run", ends):
                    asyncio.run(a.run("Fix it.", env, AgentContext()))
                names = ",".join(ef.ARMS[arm]["names"])
                self.assertIn(f"python3 {measured.SAMPLER} --out /logs/agent --names {names}", env.commands[0])
                self.assertEqual(env.commands[1:], ["harbor's run",
                                                    measured.smp.stop_script("/logs/agent", measured.STATE)])

                async def cancelled(self_, instruction, environment, context):
                    raise asyncio.CancelledError

                env = FakeEnvironment()
                with mock.patch.object(base, "run", cancelled), self.assertRaises(asyncio.CancelledError):
                    asyncio.run(a.run("Fix it.", env, AgentContext()))
                self.assertEqual(env.commands[-2:], [
                    measured.measure.stop_agent_script(ef.ARMS[arm]["names"]),
                    measured.smp.stop_script("/logs/agent", measured.STATE)])
                self.assertEqual(env.users[-2], "root")

    def test_install_reads_the_version_and_uploads_the_sampler_after_harbors(self):
        for cls, base, model, arm, _, _ in arms():
            if arm == "aider":
                continue  # its own install: below
            with self.subTest(arm):
                a = self.make(cls, model)

                async def harbors(self_, environment):
                    environment.commands.append("harbor's install")

                env = FakeEnvironment()
                with mock.patch.object(base, "install", harbors):
                    asyncio.run(a.install(env))
                self.assertEqual(env.commands[0], "harbor's install")
                self.assertIn((str(Path(measured.smp.__file__)), measured.SAMPLER), env.uploads)
                self.assertTrue(any(c.endswith("version.txt; true") for c in env.commands))
                if arm == "openhands":
                    self.assertTrue(any("ln -sf /opt/openhands-sdk-venv/bin/python "
                                        "/opt/openhands-sdk-venv/bin/openhands-py" in c for c in env.commands))
                    self.assertIn((str(Path(openhands_agent.__file__).with_name("openhands_measure_run.py")),
                                   openhands_agent.WRAPPER), env.uploads)

    def test_aiders_install_is_pinned_and_never_runs_harbors(self):
        a = self.make(aider_agent.MeasuredAider, SONNET)
        env = FakeEnvironment()

        async def harbors(self_, environment):
            raise AssertionError("Harbor's unpinned install ran")

        with mock.patch.object(Aider, "install", harbors), \
                mock.patch.object(Aider, "ensure_system_dependencies", mock.AsyncMock()):
            asyncio.run(a.install(env))
        installs = [c for c in env.commands if "uv tool install" in c]
        self.assertEqual(len(installs), 1)
        self.assertIn("aider-chat==0.86.2 ", installs[0])
        self.assertEqual(sorted(u[1] for u in env.uploads if u[1] in (aider_agent.METADATA, aider_agent.SETTINGS)),
                         sorted([aider_agent.METADATA, aider_agent.SETTINGS]))
        # No temperature (Sonnet 5.5 refuses one), the effort as output_config, one model throughout.
        s = aider_agent.settings(SONNET, "medium")[0]
        self.assertEqual((s["use_temperature"], s["extra_params"]["output_config"], s["weak_model_name"],
                          s["editor_model_name"]), (False, {"effort": "medium"}, SONNET, SONNET))
        self.assertNotIn("--reasoning-effort", a.build_cli_flags())
        self.assertEqual(aider_agent.settings(SONNET, "high")[0]["extra_params"]["output_config"], {"effort": "high"})
        entry = aider_agent.metadata(SONNET)[SONNET]
        self.assertAlmostEqual(entry["input_cost_per_token"], 2e-6, places=12)
        self.assertAlmostEqual(entry["cache_read_input_token_cost"], 2e-7, places=12)

    def test_aiders_command_names_the_provider_and_keeps_its_logs(self):
        a = self.make(aider_agent.MeasuredAider, SONNET)
        harbors = (". $HOME/.local/bin/env; aider --yes --chat-history-file=/logs/agent/aider.chat.history.md "
                   "--model=claude-sonnet-5-5 --message='Fix it.' 2>&1 | tee x")
        cmd, _ = a.rewrite(harbors, {})
        self.assertIn("--model=anthropic/claude-sonnet-5-5 ", cmd)
        for flag in ("--model-settings-file " + aider_agent.SETTINGS, "--model-metadata-file " + aider_agent.METADATA,
                     "--analytics-log " + aider_agent.ANALYTICS, "--no-analytics"):
            self.assertIn(flag, cmd)
        self.assertEqual(a.rewrite("mkdir -p /x", None), ("mkdir -p /x", None))

    def test_opencodes_run_is_offline_from_its_catalog(self):
        a = self.make(opencode_agent.MeasuredOpenCode, SONNET)
        run = ("[ -f ~/.nvm/nvm.sh ] && . ~/.nvm/nvm.sh; opencode --model=anthropic/claude-sonnet-5-5 run "
               "--format=json --variant medium --thinking --dangerously-skip-permissions -- 'x'")
        _, env = a.rewrite(run, {"ANTHROPIC_API_KEY": "k"})
        self.assertEqual(env["OPENCODE_DISABLE_MODELS_FETCH"], "1")
        self.assertEqual(env["ANTHROPIC_API_KEY"], "k")
        self.assertEqual(a.rewrite("mkdir -p ~/.config/opencode", None), ("mkdir -p ~/.config/opencode", None))

    def test_openhands_runs_under_its_own_name_through_the_wrapper(self):
        a = self.make(openhands_agent.MeasuredOpenHands, SONNET)
        cmd, _ = a.rewrite('/opt/openhands-sdk-venv/bin/python /installed-agent/run_agent.py     --instruction=x '
                           '--logs-dir="$AGENT_LOGS_DIR" 2>&1 | stdbuf -oL tee /logs/agent/openhands_sdk.txt', {})
        self.assertIn(f"/opt/openhands-sdk-venv/bin/openhands-py {openhands_agent.WRAPPER}     --instruction=x", cmd)
        self.assertNotIn("bin/python /installed-agent", cmd)

    def test_openclaws_own_timeout_never_comes_before_the_tasks(self):
        a = self.make(openclaw_agent.MeasuredOpenClaw, SONNET)
        self.assertIn("--timeout 14400", a.build_cli_flags())
        self.assertIn("--thinking medium", a.build_cli_flags())

    def test_openclaw_runs_on_node_24_as_its_release_asks(self):
        a = self.make(openclaw_agent.MeasuredOpenClaw, SONNET)
        env = FakeEnvironment()

        async def none(*args, **kwargs):
            return None

        with mock.patch.object(OpenClaw, "ensure_system_dependencies", none):
            asyncio.run(OpenClaw.install(a, env))
        joined = " ".join(env.commands)
        self.assertIn("nvm install 24", joined)
        self.assertIn("nvm use 24", joined)
        self.assertNotIn("nvm use 22", joined)
        self.assertNotIn("nvm install 22", joined)
        self.assertIn("openclaw@2026.9.8", joined)
        self.assertIn("nvm use 24", a.get_version_command())

    def test_the_record_lands_in_the_context_and_prices_an_unpriced_trial(self):
        (self.logs / "opencode.txt").write_text(OPENCODE)
        (self.logs / "version.txt").write_text("1.18.35\n")
        a = self.make(opencode_agent.MeasuredOpenCode, SONNET)
        ctx = AgentContext()
        with mock.patch.object(OpenCode, "populate_context_post_run", lambda self_, c: None):
            a.populate_context_post_run(ctx)
        rec = json.loads((self.logs / ef.RECORD).read_text())
        self.assertEqual(ctx.metadata["efficiency"], rec)
        self.assertEqual((rec["effort"], rec["version"], rec["version_asked"]), ("medium", "1.18.35", "1.18.35"))
        self.assertEqual((rec["limits"]["enforced"], rec["limits"]["over_budget"]), (False, False))
        self.assertAlmostEqual(ctx.cost_usd, 0.0139)
        self.assertEqual(ctx.n_input_tokens, 15 + 2000 + 2300)

    def test_a_broken_record_never_fails_the_trial(self):
        a = self.make(codex_agent.MeasuredCodex, codex_agent.NATIVE_MODEL)
        ctx = AgentContext()
        with mock.patch.object(codex_agent.ef, "codex_record", side_effect=ValueError("bad")):
            a.populate_context_post_run(ctx)
        self.assertEqual(ctx.metadata["efficiency"]["error"], "ValueError: bad")


if __name__ == "__main__":
    unittest.main()

"""Tests for the measured Claude Code arm (theseus-7gir.12).

    .venv/bin/python -m unittest discover -s bench/harbor

They need Harbor (its venv's python) and skip on the standard library.
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
import test_efficiency as fx

try:
    from harbor.agents.installed.claude_code import ClaudeCode
    from harbor.environments.base import ExecResult
    from harbor.models.agent.context import AgentContext

    import claude_code_agent as cca
except ImportError:  # Harbor is not installed: these skip.
    ClaudeCode = None


class FakeEnvironment:
    """What the arm asks of a container: each command, as it was sent."""

    default_user = "agent"

    def __init__(self):
        self.commands: list[str] = []
        self.users: list[str | None] = []
        self.uploads: list[tuple[str, str]] = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        self.commands.append(command)
        self.users.append(user)
        return ExecResult(stdout="", stderr="", return_code=0)

    async def upload_file(self, source_path, target_path):
        self.uploads.append((str(source_path), target_path))


@unittest.skipIf(ClaudeCode is None, "Harbor is not installed")
class Arm(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.logs = Path(self.tmp.name)
        self.agent = cca.MeasuredClaudeCode(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5",
                                            max_turns=200)

    def tearDown(self):
        self.tmp.cleanup()

    def test_it_is_harbors_claude_code_by_name_and_options(self):
        self.assertTrue(issubclass(cca.MeasuredClaudeCode, ClaudeCode))
        self.assertEqual(cca.MeasuredClaudeCode.name(), ClaudeCode.name())
        self.assertIs(cca.MeasuredClaudeCode.options_model, ClaudeCode.options_model)
        self.assertEqual(self.agent.options.max_turns, 200)
        self.assertEqual(cca.ARM["names"], ("claude",))

    def run_with(self, harbors_run):
        env = FakeEnvironment()
        with mock.patch.object(ClaudeCode, "run", harbors_run):
            asyncio.run(self.agent.run("Fix the repository.", env, AgentContext()))
        return env

    def test_the_sampler_runs_around_harbors_run(self):
        seen = []

        async def harbors(self_, instruction, environment, context):
            seen.append((instruction, list(environment.commands)))

        env = self.run_with(harbors)
        self.assertEqual(seen[0][0], "Fix the repository.")
        self.assertEqual(len(seen[0][1]), 1)
        self.assertIn(f"python3 {cca.SAMPLER} --out /logs/agent --names claude", seen[0][1][0])
        self.assertEqual(env.commands[-1], cca.smp.stop_script("/logs/agent", cca.STATE))

    def test_harbors_timeout_still_stops_the_sampler(self):
        """The agent first, as root (`claude`, and what it started), then the
        sampler, in that order, and the cancel is raised again: Harbor's
        timeout still ends the trial as a timeout (theseus-sgpx)."""
        async def cancelled(self_, instruction, environment, context):
            environment.commands.append("claude, cancelled")
            raise asyncio.CancelledError

        env = FakeEnvironment()
        with mock.patch.object(ClaudeCode, "run", cancelled):
            with self.assertRaises(asyncio.CancelledError):
                asyncio.run(self.agent.run("Fix it.", env, AgentContext()))
        self.assertEqual(env.commands[-3:], ["claude, cancelled", cca.measure.stop_agent_script(("claude",)),
                                             cca.smp.stop_script("/logs/agent", cca.STATE)])
        self.assertEqual(env.users[-2], "root")

    def test_a_run_that_ends_by_itself_stops_no_agent(self):
        async def harbors(self_, instruction, environment, context):
            pass

        env = self.run_with(harbors)
        self.assertEqual(len(env.commands), 2)
        self.assertNotIn(cca.measure.stop_agent_script(("claude",)), env.commands)

    def test_the_effort_is_medium_and_a_hosts_environment_does_not_pick_it(self):
        self.assertEqual(cca.measure.EFFORT, "medium")
        with mock.patch.dict(os.environ, {"CLAUDE_CODE_EFFORT_LEVEL": "max"}):
            agent = cca.MeasuredClaudeCode(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5")
            self.assertEqual(agent.options.reasoning_effort, "medium")
            self.assertEqual(ClaudeCode(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5")
                             .options.reasoning_effort, "max")  # Harbor's own takes the host's
        self.assertIn("--effort medium", self.agent.build_cli_flags())
        # An explicit --ak is an ablation, and wins.
        high = cca.MeasuredClaudeCode(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5",
                                      reasoning_effort="high")
        self.assertEqual(high.options.reasoning_effort, "high")
        self.assertIn("--effort high", high.build_cli_flags())

    def test_the_install_is_pinned_on_both_of_harbors_branches(self):
        self.assertEqual(cca.PINNED_VERSION, "2.1.290")
        env = FakeEnvironment()
        asyncio.run(self.agent.install(env))
        installs = [c for c in env.commands if "bootstrap.sh" in c]
        self.assertEqual(len(installs), 1)
        # The native installer's (a glibc image) and npm's (Alpine).
        self.assertIn("bootstrap.sh | bash -s -- 2.1.290;", installs[0])
        self.assertIn("npm install -g @anthropic-ai/claude-code@2.1.290;", installs[0])
        self.assertNotIn("claude-code;", installs[0])
        self.assertEqual(self.agent.version(), "2.1.290")
        # A version named for the trial wins over the pin, on both branches.
        other = cca.MeasuredClaudeCode(logs_dir=self.logs, model_name="anthropic/claude-sonnet-5-5",
                                       version="2.1.288")
        env = FakeEnvironment()
        asyncio.run(other.install(env))
        install = [c for c in env.commands if "bootstrap.sh" in c][0]
        self.assertIn("bash -s -- 2.1.288;", install)
        self.assertIn("claude-code@2.1.288;", install)
        self.assertNotIn("2.1.290", install)

    def test_the_container_reads_the_version_into_the_logs(self):
        env = FakeEnvironment()
        asyncio.run(self.agent.install(env))
        want = cca.measure.version_script(self.agent.get_version_command(), "/logs/agent")
        self.assertEqual([c for c in env.commands if c.endswith("version.txt; true")], [want])
        self.assertIn('claude --version 2>/dev/null )', want)

    def test_the_version_the_container_read_is_named_in_the_record(self):
        (self.logs / "version.txt").write_text("2.1.288 (Claude Code)\n")
        self.agent.populate_context_post_run(AgentContext())
        rec = json.loads((self.logs / ef.RECORD).read_text())
        self.assertEqual((rec["version"], rec["version_asked"]), ("2.1.288", "2.1.290"))

    def test_install_uploads_the_sampler_after_harbors_install(self):
        env = FakeEnvironment()

        async def harbors(self_, environment):
            environment.commands.append("harbor's install")

        with mock.patch.object(ClaudeCode, "install", harbors):
            asyncio.run(self.agent.install(env))
        self.assertEqual(env.commands[0], "harbor's install")
        self.assertEqual(env.uploads, [(str(Path(cca.smp.__file__)), cca.SAMPLER)])
        self.assertIn("chown agent", env.commands[-1])

    def test_the_record_follows_harbors_own_context(self):
        (self.logs / "claude-code.txt").write_text(fx.STREAM)
        proj = self.logs / "sessions" / "projects" / "-app"
        proj.mkdir(parents=True)
        (proj / "ses.jsonl").write_text("\n".join(json.dumps(e) for e in fx.SESSION) + "\n")
        (self.logs / "sampler.json").write_text(json.dumps(fx.SUMMARY))
        ctx = AgentContext()
        self.agent.populate_context_post_run(ctx)
        # Harbor's own: its trajectory and its three counters, from the log.
        self.assertTrue((self.logs / "trajectory.json").exists())
        self.assertEqual(ctx.cost_usd, 0.0391)
        rec = json.loads((self.logs / ef.RECORD).read_text())
        self.assertEqual(ctx.metadata["efficiency"], rec)
        self.assertEqual((rec["arm"], rec["spend_from"], rec["model_calls"], rec["tool_calls"]),
                         ("claude-code", "result_event", 2, 2))
        self.assertEqual(rec["tokens"]["cache_write"], 4200)
        self.assertEqual(rec["harness"]["cpu_s"], 1.2)
        # What it asked for, and the container's own version read (none was
        # kept here): the keys every arm's record has.
        self.assertEqual((rec["effort"], rec["version"], rec["version_asked"]), ("medium", None, "2.1.290"))
        # Harbor's converter counts each message once too: the same calls.
        traj = json.loads((self.logs / "trajectory.json").read_text())
        self.assertEqual(ef.trajectory_spend(traj)["model_calls"], 2)
        self.assertEqual(ef.trajectory_spend(traj)["tokens"]["cache_write"], 4200)


if __name__ == "__main__":
    unittest.main()

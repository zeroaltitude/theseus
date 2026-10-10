"""Tests for the arms' scratch setups (theseus-7gir.13): Claude Code's home and Theseus's config, both only ever on
a stand-in on 127.0.0.1.

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import json
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import claude_code as cc  # noqa: E402
import theseus as th  # noqa: E402

URL = "http://127.0.0.1:45123"


class ClaudeCode(unittest.TestCase):
    def test_the_scratch_home_is_onboarded_trusted_and_local(self):
        with tempfile.TemporaryDirectory() as d:
            root, work = Path(d) / "cc", Path(d) / "work"
            env = cc.scratch(root, work, URL)
            state = json.loads((Path(env["CLAUDE_CONFIG_DIR"]) / ".claude.json").read_text())
            settings = json.loads((Path(env["CLAUDE_CONFIG_DIR"]) / "settings.json").read_text())
        self.assertTrue(state["hasCompletedOnboarding"])
        self.assertTrue(state["projects"][str(work)]["hasTrustDialogAccepted"])
        self.assertEqual(state["customApiKeyResponses"]["approved"], [cc.DUMMY_KEY[-20:]])
        self.assertTrue(settings["skipDangerousModePermissionPrompt"])
        self.assertEqual(env["ANTHROPIC_BASE_URL"], URL)
        for k in ("DISABLE_AUTOUPDATER", "DISABLE_TELEMETRY", "DISABLE_ERROR_REPORTING",
                  "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"):
            self.assertEqual(env[k], "1")
        self.assertFalse(any("PROXY" in k and k != "NO_PROXY" for k in env))

    def test_a_remote_endpoint_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(ValueError):
                cc.scratch(Path(d), Path(d), "https://api.anthropic.com")

    def test_the_command_lines(self):
        self.assertIn("--dangerously-skip-permissions", cc.interactive_argv("claude"))
        self.assertEqual(cc.interactive_argv("claude", "abc")[-2:], ["--resume", "abc"])
        self.assertEqual(cc.oneshot_argv("claude", "hi", session_id="u1")[1:3], ["-p", "hi"])


class Theseus(unittest.TestCase):
    def test_the_config_is_the_bench_profiles_on_the_stand_in(self):
        t = tomllib.loads(th.config(URL, Path("/w")))
        self.assertEqual(t["model"]["api_base"], URL)
        self.assertEqual(t["secrets"]["anthropic_api_key"], f"env:{th.KEY_ENV}")
        self.assertEqual(t["policy"]["enforcement"], "open")
        for off in ("discord", "web", "index"):
            self.assertFalse(t[off]["enabled"], off)
        for absent in ("memory", "lsp", "judge"):  # off by default, so not in the config at all
            self.assertNotIn(absent, t)
        self.assertEqual(t["tools"]["projects_dir"], "/w")

    def test_a_remote_endpoint_is_refused(self):
        with self.assertRaises(ValueError):
            th.config("https://api.anthropic.com", Path("/w"))

    def test_the_surfaces(self):
        d = th.Daemon.__new__(th.Daemon)
        d.bin_dir, d.socket = Path("/b"), Path("/s/theseus.sock")
        self.assertEqual(th.tui_argv(d), ["/b/theseus-tui", "--socket", "/s/theseus.sock", "--notify", "off"])
        self.assertEqual(th.ask_argv(d, "hi", "ses_1"), ["/b/theseus", "--socket", "/s/theseus.sock", "ask", "-s",
                                                         "ses_1", "hi"])


if __name__ == "__main__":
    unittest.main()

"""The head-to-head bench's Claude Code arm (theseus-7gir.13): a pinned Claude Code, in a scratch home, talking only
to the stand-in.

Every run gets its own HOME and CLAUDE_CONFIG_DIR, marked onboarded, its work directory trusted and the bypass mode's
warning accepted, so no dialog stands between a start and its prompt; ANTHROPIC_BASE_URL points at the stand-in on
127.0.0.1 and the key is a dummy, so nothing leaves the machine. Updates, telemetry, error reports and every other
nonessential request are off. Interactive runs take `--dangerously-skip-permissions` (Theseus's arm runs its tools
open too); one-shots take `-p`.

The ready prompt (T1, T5) is the input box's footer, "bypass permissions on (shift+tab to cycle)", which Claude Code
draws under its prompt once the prompt takes keys (without the bypass mode the same line reads "? for shortcuts").
Auto mode is turned off in the scratch settings, so its first-run offer never covers the prompt.

Standard library only.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from pathlib import Path

CLAUDE_CODE_VERSION = "2.1.296"
MODEL = "claude-sonnet-5-5"
DUMMY_KEY = "sk-ant-h2h-standin-0000000000000000000000"
READY_MARKER = "bypass permissions on"
# The only variables of the bench's own environment a run sees: no proxy, no other credentials, no host's tokens.
KEEP_ENV = ("PATH", "LANG", "LC_ALL", "TERM", "TZ", "USER", "LOGNAME", "SHELL")
ARM = "claude-code"


def find(path: str | None = None) -> str | None:
    """The claude binary: the one given, else the one on PATH; None when there is none."""
    return path if path and os.access(path, os.X_OK) else shutil.which("claude")


def version(binary: str) -> str | None:
    """`claude --version`'s number (it needs no network)."""
    try:
        out = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=30).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    m = re.search(r"\d+\.\d+\.\d+", out)
    return m.group(0) if m else None


def scratch(root: Path, work: Path, base_url: str) -> dict[str, str]:
    """Write a scratch home under `root` for one arm's runs and return the environment to run Claude Code in."""
    if not re.match(r"^http://127\.0\.0\.1:\d+", base_url):
        raise ValueError(f"the stand-in must be on 127.0.0.1, not {base_url!r}: this arm never reaches a real API")
    home, conf = root / "home", root / "home" / ".claude"
    conf.mkdir(parents=True, exist_ok=True)
    project = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True, "allowedTools": [],
               "projectOnboardingSeenCount": 1, "hasClaudeMdExternalIncludesApproved": False}
    state = {
        "hasCompletedOnboarding": True,
        "lastOnboardingVersion": CLAUDE_CODE_VERSION,
        "bypassPermissionsModeAccepted": True,
        "numStartups": 1,
        "theme": "dark",
        "autoUpdates": False,
        "hasResetAutoModeOptInForDefaultOffer": True,
        "customApiKeyResponses": {"approved": [DUMMY_KEY[-20:]], "rejected": []},
        "projects": {str(work): project, str(work.resolve()): project},
    }
    for p in (conf / ".claude.json", home / ".claude.json"):
        p.write_text(json.dumps(state, indent=1))
    (conf / "settings.json").write_text(json.dumps({
        "skipDangerousModePermissionPrompt": True,
        "permissions": {"defaultMode": "bypassPermissions", "disableAutoMode": "disable"},
        "autoUpdates": False,
    }, indent=1))
    env = {k: os.environ[k] for k in KEEP_ENV if k in os.environ}
    env.update({
        "HOME": str(home),
        "CLAUDE_CONFIG_DIR": str(conf),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_STATE_HOME": str(home / ".local" / "state"),
        "ANTHROPIC_BASE_URL": base_url,
        "ANTHROPIC_API_KEY": DUMMY_KEY,
        "ANTHROPIC_MODEL": MODEL,
        "ANTHROPIC_SMALL_FAST_MODEL": MODEL,
        "DISABLE_AUTOUPDATER": "1",
        "DISABLE_TELEMETRY": "1",
        "DISABLE_ERROR_REPORTING": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
        "NO_PROXY": "127.0.0.1,localhost",
    })
    if os.geteuid() == 0:  # Claude Code refuses the bypass mode to root outside a sandbox; a bench box may be one
        env["IS_SANDBOX"] = "1"
    return env


def interactive_argv(binary: str, resume: str | None = None) -> list[str]:
    argv = [binary, "--dangerously-skip-permissions", "--model", MODEL]
    return argv + (["--resume", resume] if resume else [])


def oneshot_argv(binary: str, prompt: str, session_id: str | None = None, resume: str | None = None) -> list[str]:
    """One turn: `-p`, a fresh session named `session_id` (a UUID), or `resume` one."""
    argv = [binary, "-p", prompt, "--dangerously-skip-permissions", "--model", MODEL]
    if session_id:
        argv += ["--session-id", session_id]
    if resume:
        argv += ["--resume", resume]
    return argv

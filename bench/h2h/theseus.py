"""The head-to-head bench's Theseus arm (theseus-7gir.13): scratch daemons of a release build, on the stand-in.

Each daemon gets its own state dir, socket and config, never the operator's: the config holds only what differs from
the defaults, as the bench profile does (bench/theseus-bench.toml), with the model's `api_base` on the stand-in and
its key a dummy from the environment (`env:H2H_STANDIN_KEY`). Discord, the web UI and the index are off; memory
(recall), the judge and the language servers are off by default and stay so; every tool is open, as Claude Code's
arm runs with its permissions skipped.

The surfaces:
- interactive: `theseus-tui --socket S --notify off`. It shows the daemon's sessions; it opens none. So a session is
  opened first (`theseus sessions open --label h2h-...`), and the TUI's ready prompt (T1) is the board with that
  session on it and the link up ("socket ok"); then Enter opens the session, `i` the input line (" > "), and the
  prompt and Enter send a turn.
- one-shot: `theseus --socket S ask -s <session> "<prompt>"` (a fresh session without `-s`).
- resume (T5): a session made long by turns through the stand-in, then the TUI opening it to its last reply on
  screen, and `ask -s` on it to its request.

T1 for Theseus is the surface's start with the daemon already serving, as Theseus is used (the daemon is a service);
the daemon's own start to serving is recorded beside it (`daemon_start_ms`), never added in.

Standard library only.
"""

from __future__ import annotations

import atexit
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

ARM = "theseus"
MODEL = "claude-sonnet-5-5"
KEY_ENV = "H2H_STANDIN_KEY"
READY_MARKERS = ("socket ok",)  # plus the session's label, which the bench adds
# The input line's prompt, " > " on the footer row. The TUI moves the cursor to draw it, so its leading space is not
# in the transcript: the bench looks for ">" in what the TUI draws after the keypress that opens the line.
INPUT_MARKER = ">"
KEEP_ENV = ("PATH", "LANG", "LC_ALL", "TERM", "TZ", "USER", "LOGNAME", "SHELL")


def config(base_url: str, work: Path) -> str:
    """The scratch daemon's config: the bench profile's choices, on the stand-in."""
    if not base_url.startswith("http://127.0.0.1:"):
        raise ValueError(f"the stand-in must be on 127.0.0.1, not {base_url!r}")
    return f"""# The head-to-head bench's scratch daemon (bench/h2h/theseus.py): only what differs from the defaults.
[model]
live = "h2h"
provider = "anthropic"
model = "{MODEL}"
api_base = "{base_url}"
api_key_secret = "anthropic_api_key"

[profiles.h2h]
provider = "anthropic"
model = "{MODEL}"
effort = "medium"

[secrets]
anthropic_api_key = "env:{KEY_ENV}"

[server]
disk_warn_mb = 0
disk_floor_mb = 0

[tools]
projects_dir = "{work}"
roots = ["/"]
approve_paths = []

[policy]
enforcement = "open"
allow_argv = []
approve_argv = []
external_text = "notify"
external_programs = []
private_addresses = "open"

[sandbox]
default = "l0"

[discord]
enabled = false

[web]
enabled = false

[index]
enabled = false
"""


def env_for(root: Path) -> dict[str, str]:
    """A hermetic environment: a scratch HOME and XDG dirs (the TUI's seen file lands there), the dummy key."""
    home = root / "home"
    home.mkdir(parents=True, exist_ok=True)
    env = {k: os.environ[k] for k in KEEP_ENV if k in os.environ}
    env.update({"HOME": str(home), "XDG_STATE_HOME": str(home / ".local" / "state"),
                "XDG_CONFIG_HOME": str(home / ".config"), KEY_ENV: "sk-ant-h2h-standin-0000",
                "NO_PROXY": "127.0.0.1,localhost"})
    return env


class Daemon:
    """One scratch `theseusd` under `root`: its config, state dir and socket."""

    def __init__(self, bin_dir: Path, root: Path, base_url: str, work: Path) -> None:
        self.bin_dir, self.root = Path(bin_dir), Path(root)
        # A socket's path must fit in sun_path (108 bytes): the daemon's socket, and the spool's notify socket under
        # its state dir (too long, and a job's completion waits for the daemon's 1 s heartbeat instead), so both
        # live in a short directory of their own.
        short = Path(tempfile.mkdtemp(prefix="h2h-"))
        atexit.register(shutil.rmtree, short, True)
        self.state, self.socket = short / "state", short / "sock"
        self.conf = self.root / "theseus.toml"
        self.root.mkdir(parents=True, exist_ok=True)
        self.conf.write_text(config(base_url, work))
        self.env = env_for(self.root)
        self.work = work
        self.proc: subprocess.Popen | None = None
        self.start_ms: float | None = None

    def bin(self, name: str) -> str:
        return str(self.bin_dir / name)

    def start(self, timeout: float = 30.0) -> float:
        """Start it and wait until it answers health; returns spawn -> first answer in ms."""
        t0 = time.monotonic_ns()
        self.proc = subprocess.Popen([self.bin("theseusd"), "--config", str(self.conf), "--socket", str(self.socket),
                                      "--state-dir", str(self.state)], env=self.env, cwd=str(self.work),
                                     stdout=open(self.root / "theseusd.log", "ab"), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"theseusd exited {self.proc.returncode}: see {self.root / 'theseusd.log'}")
            if self.socket.exists() and self.cli("health", check=False, timeout=5).returncode == 0:
                self.start_ms = (time.monotonic_ns() - t0) / 1e6
                return self.start_ms
            time.sleep(0.005)
        raise RuntimeError("theseusd did not answer health in time")

    @property
    def pid(self) -> int | None:
        return self.proc.pid if self.proc else None

    def cli(self, *args: str, check: bool = True, timeout: float = 120) -> subprocess.CompletedProcess:
        return subprocess.run([self.bin("theseus"), "--socket", str(self.socket), *args], env=self.env,
                              cwd=str(self.work), capture_output=True, text=True, timeout=timeout, check=check)

    def open_session(self, label: str) -> str:
        return self.cli("sessions", "open", "--label", label).stdout.strip().split()[-1]

    def version(self) -> str | None:
        out = subprocess.run([self.bin("theseusd"), "--version"], capture_output=True, text=True).stdout
        return out.strip() or None

    def stop(self) -> None:
        if not self.proc or self.proc.poll() is not None:
            return
        self.cli("shutdown", check=False, timeout=10)
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()  # this daemon's own pid, which the bench started
            self.proc.wait()


def tui_argv(d: Daemon) -> list[str]:
    return [d.bin("theseus-tui"), "--socket", str(d.socket), "--notify", "off"]


def ask_argv(d: Daemon, prompt: str, session: str | None = None) -> list[str]:
    argv = [d.bin("theseus"), "--socket", str(d.socket), "ask"]
    return argv + (["-s", session] if session else []) + [prompt]

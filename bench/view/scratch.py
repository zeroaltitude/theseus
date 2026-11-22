"""The measuring kit's scratch daemon (issue 44, §10 of the context design): a
`theseusd` on a COPY of a store, which the kit's tools read and nothing else.

    with Scratch(bin_dir, out, store=copy) as d:     # a copy of a copy: the one given is never written
        d.cli("--json", "view", "preview", place)
    with Scratch(bin_dir, out) as d:                 # an empty store (the labelled cases build theirs)

How it is kept apart from the owner's daemon, as bench/recall's driver keeps
an arm's (`drive.py`, `Theseus`):

- **its own state, socket and config** under `out/daemon/`, passed on the
  command line; `THESEUS_CONFIG`, `THESEUS_STATE_DIR`, `THESEUS_SOCKET` and
  `THESEUS_SESSION` are taken out of its environment and the CLI's, and the
  CLI's seen file is the run's (`XDG_STATE_HOME`);
- **never the live store**: `store` is refused when it is the daemon's default
  state directory (`~/.theseus`) or `$THESEUS_STATE_DIR`, when a running
  process names it (a daemon serves it), and when it holds no `store/wal`
  (it is a daemon's STATE directory, copied). It is then
  copied whole into `out/daemon/state`, and the daemon runs on that, so even
  the copy it was given is left as it was (a read writes a ledger row); that
  copy is removed when the daemon stops;
- **no money**: the bench profile with every provider pointed at an address
  nothing listens on (`DEAD_API`) and a key that is no key, the spend limit
  and the day's ceiling at a cent with the ceiling's mode `stop`; Discord,
  the web UI, the MCP server and the index off. A background job that would
  call a model (the ladder's `did` lines) fails instead of spending;
- **nothing left running**: `shutdown`, a kill after `STOP_WAIT`, then
  whatever still names `out/daemon` (bench/recall's `processes_naming`).

Standard library only.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE.parent / "recall"))

import drive  # noqa: E402

PROFILE = ROOT / "bench" / "theseus-bench.toml"
# An address nothing listens on: a model call fails at once and costs nothing.
DEAD_API = "http://127.0.0.1:9"
NO_KEY = "no-key-the-kit-calls-no-model"
CENT = 0.01
SERVE_WAIT = drive.SERVE_WAIT
STOP_WAIT = drive.STOP_WAIT
SCRUBBED_ENV = ("THESEUS_CONFIG", "THESEUS_STATE_DIR", "THESEUS_SOCKET", "THESEUS_SESSION", "ANTHROPIC_API_KEY",
                "TYPESAFE_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "ZAI_API_KEY")


class Refused(SystemExit):
    """The kit will not run on what it was given: the message says why."""


def live_dirs(env: dict | None = None) -> list[Path]:
    """The state directories a daemon of this machine may be serving: the
    default and the environment's."""
    env = os.environ if env is None else env
    out = [Path("~/.theseus").expanduser()]
    if env.get("THESEUS_STATE_DIR"):
        out.append(Path(env["THESEUS_STATE_DIR"]).expanduser())
    return [p.resolve() for p in out]


def check_copy(store: Path, env: dict | None = None) -> Path:
    """`store`, resolved, or `Refused`: it must be a copy no daemon serves."""
    store = Path(store).expanduser()
    if not store.is_dir():
        raise Refused(f"view kit: {store} is not a directory: give it a COPY of a store")
    store = store.resolve()
    for live in live_dirs(env):
        if store == live or live in store.parents or store in live.parents:
            raise Refused(f"view kit: {store} is a live state directory: copy it first "
                          "(cp -a, with its daemon stopped or from a backup) and give the kit the copy")
    if drive.processes_naming(store):
        raise Refused(f"view kit: a running process names {store}: a daemon serves it; give the kit a copy")
    if not (store / "store" / "wal").is_dir():
        raise Refused(f"view kit: {store} holds no store/wal: give it a copy of a daemon's state directory")
    return store


# The kernel's ledger of a STAND-IN model's calls (a local process that charges nothing): it reserves each
# call's worst case at list price before the call, so a cent would refuse the first one.
STAND_IN_LIMIT = 50.0


def config(base: dict, api_base: str = DEAD_API, model: str | None = None) -> dict:
    """The scratch daemon's config: the bench profile, nothing that spends,
    nothing that listens. `api_base` is the stand-in model's address when the
    labelled cases build a store (a local process: it charges nothing, and the
    kernel's limit is then `STAND_IN_LIMIT`, of list-price bookkeeping only)."""
    if not api_base.startswith("http://127.0.0.1:"):
        raise ValueError(f"view kit: the scratch daemon's model is a local stand-in or none, not {api_base}")
    t = json.loads(json.dumps(base))
    if model:
        t.setdefault("model", {})["model"] = model
        t.setdefault("profiles", {}).setdefault("bench", {})["model"] = model
    t.setdefault("model", {})["api_base"] = api_base
    for p in t.get("providers", {}).values():
        p["api_base"] = api_base
    k = t.setdefault("kernel", {})
    limit = CENT if api_base == DEAD_API else STAND_IN_LIMIT
    k["spend_limit_usd"] = limit
    k["daily_spend_ceiling_usd"] = limit
    k["daily_spend_ceiling_mode"] = "stop"
    t.setdefault("discord", {})["enabled"] = False
    t.setdefault("web", {})["enabled"] = False
    if "mcp_server" in t:
        t["mcp_server"]["enabled"] = False
    t.setdefault("index", {})["enabled"] = False
    return t


class Scratch:
    def __init__(self, bin_dir: Path, out: Path, store: Path | None = None, api_base: str = DEAD_API,
                 model: str | None = None, env: dict | None = None):
        bins = Path(bin_dir).resolve()
        self.theseus, self.theseusd = bins / "theseus", bins / "theseusd"
        for b in (self.theseus, self.theseusd):
            if not b.is_file():
                raise SystemExit(f"view kit: no {b}")
        self.dir = Path(out).resolve() / "daemon"
        self.dir.mkdir(parents=True, exist_ok=False)
        self.state, self.sock, self.cfg = self.dir / "state", self.dir / "sock", self.dir / "config.toml"
        self.log = self.dir / "theseusd.log"
        # The copy it made of a store is removed when it stops: a run leaves counts, and the daemon's log.
        self.copied = store is not None
        if store is not None:
            shutil.copytree(check_copy(store, env), self.state, symlinks=True)
        self.cfg.write_text(drive.toml_dumps(config(tomllib.loads(PROFILE.read_text()), api_base, model)))
        self.env = drive._env_without(SCRUBBED_ENV)
        self.env["ANTHROPIC_API_KEY"] = NO_KEY
        self.proc: subprocess.Popen | None = None

    def cli(self, *args: str, input: str | None = None, timeout: float = 120.0,
            cwd: Path | None = None) -> subprocess.CompletedProcess:
        return subprocess.run([str(self.theseus), "--socket", str(self.sock), *args], input=input,
                              capture_output=True, text=True, timeout=timeout, cwd=cwd,
                              env={**self.env, "XDG_STATE_HOME": str(self.dir / "xdg-state")})

    def json(self, *args: str, **kw):
        """`theseus --json <args>`'s value, or RuntimeError with its first line of stderr."""
        r = self.cli("--json", *args, **kw)
        if r.returncode != 0:
            raise RuntimeError(f"theseus {args[0]} failed: {(r.stderr.strip().splitlines() or [''])[0][:200]}")
        return json.loads(r.stdout)

    def start(self) -> "Scratch":
        with self.log.open("w") as log:
            self.proc = subprocess.Popen([str(self.theseusd), "--config", str(self.cfg), "--state-dir",
                                          str(self.state), "--socket", str(self.sock)],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log,
                                         env=self.env)
        t0 = time.monotonic()
        while True:
            if self.proc.poll() is not None:
                raise RuntimeError(f"theseusd exited ({self.proc.returncode}) before serving: see {self.log}")
            try:
                if self.cli("health", timeout=10).returncode == 0:
                    return self
            except subprocess.TimeoutExpired:
                pass
            if time.monotonic() - t0 > SERVE_WAIT:
                raise RuntimeError(f"theseusd did not serve in time: see {self.log}")
            time.sleep(0.05)

    def stop(self) -> list[str]:
        killed: list[str] = []
        if self.proc is None:
            return killed
        try:
            self.cli("shutdown", timeout=10)
        except subprocess.TimeoutExpired:
            pass
        try:
            self.proc.wait(STOP_WAIT)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            killed.append(f"theseusd {self.proc.pid}")
        self.proc = None
        t0 = time.monotonic()
        while left := drive.processes_naming(self.dir):
            if time.monotonic() - t0 > STOP_WAIT:
                for pid, prog in left:
                    try:
                        os.kill(pid, 9)
                    except ProcessLookupError:
                        pass
                    killed.append(f"{prog} {pid}")
                break
            time.sleep(0.05)
        if self.copied:
            shutil.rmtree(self.state, ignore_errors=True)
        return killed

    def __enter__(self) -> "Scratch":
        return self.start()

    def __exit__(self, *exc) -> None:
        self.stop()


def ledger_rows(d: Scratch, kind: str) -> list[dict]:
    """Every row of `kind`, oldest first, paged whole (bench/recall's `ledger_pages`)."""
    return drive.ledger_pages(d.cli, kind)

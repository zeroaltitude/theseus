"""The measuring kit's scratch daemon (issue 44, §10 of the context design): a
`theseusd` on a COPY of a store, which the kit's tools read and nothing else.

    out = check_out(out, store)                     # before ANY write
    with Scratch(bin_dir, out, store=copy) as d:    # a copy of a copy: the one given is never written
        d.json("view", "preview", place)
    with Scratch(bin_dir, out) as d:                # an empty store (the labelled cases build theirs)


How it is kept apart from the owner's daemon:

- **never a live store, never written beside one**: `check_copy` refuses
  `~/.theseus`, `$THESEUS_STATE_DIR`, anything inside or above either, a
  directory a running process names, a directory with no `store/wal`, and
  a directory holding any link that leads out of it. `check_out` refuses an
  output directory inside or above a live root or the input, after
  resolving links, and runs before anything is written. The copy is made
  with links followed (none can lead out, by the check), so it holds none;
- **its own state, socket and config** under `out/daemon/` (mode 0700),
  passed on the command line;
- **a minimal environment**, built from an allow-list (`ENV_KEPT`: the path
  and the locale), with `HOME` and the XDG directories inside
  `out/daemon/`: no vault token, no token file, no provider key reaches the
  daemon or the CLI. The bench profile's key is `NO_KEY`;
- **no money**: every provider pointed at an address nothing listens on
  (`DEAD_API`), the spend limit and the day's ceiling at a cent with the
  ceiling's mode `stop`; Discord, the web UI, the MCP server and the index
  off;
- **its own words are the owner's**: the daemon decides whose a CLI message
  is by the process that sent it (`peer_proc`): an agent's when an agent
  host is above it, or when it has no controlling terminal. The kit's
  scratch daemon names no agent host (`[policy] agent_processes = []`), and
  the case builder sends each line from a terminal of the kit's own
  (`cli(..., terminal=True)`), so a synthetic line is marked the owner's
  wherever the kit runs: under an agent, a service or a cron. Without it
  his words elsewhere (slot F) hold nothing, and every `callback_elsewhere`
  case is missed for that reason alone. The daemon's rule is not changed;
- **no words in diagnostics**: the daemon's stderr is discarded, and every
  failure the kit raises is a fixed code (`Failed`), never a response, a
  row or an event;
- **nothing left behind**: on a normal end, a failed construction, a failed
  start, a health timeout, an exception or SIGTERM, the daemon is stopped
  (`shutdown`; whatever that raises or however long it hangs, then a kill of
  the daemon's own process group while the kit still holds the unreaped
  child) and the copy is removed; a removal that fails is printed to stderr
  with its path. The teardown runs to its end whatever one step raises; a
  kill or reap that failed is then `E_CLEANUP <scratch directory> pid <pid>`. SIGTERM is trapped from
  before the copy starts until the removal has finished: during the removal
  it is held, and raised once the removal is done;
- **what a SIGKILL leaves is for a person to remove**: the scratch
  directory's absolute path is the first line every run prints (on stderr);
  the store copy in it is removed at a run's end, the directory stays.
  A run killed with SIGKILL leaves that directory, which holds the copy, and
  maybe its daemon; the kit has no verb that deletes or kills on its own
  (the README says how to check and remove it by hand).

Standard library only.
"""

from __future__ import annotations

import contextlib
import json
import os
import shutil
import signal
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
# The only variables of the caller's environment a child is given.
ENV_KEPT = ("PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ")
# How long the daemon is given to answer `shutdown`, and then to exit, before its process group is killed.
SHUTDOWN_WAIT = 10.0
# The kit's own verbs, the only words a failure code may carry.
VERBS = {"view", "rpc", "history", "ask", "sessions", "health", "shutdown"}
LEDGER_PAGE = drive.LEDGER_PAGE
LEDGER_MAX = drive.LEDGER_MAX


SIGTERM_EXIT = "view kit: E_SIGTERM: stopped; the daemon is stopped and the copy removed"


class Refused(SystemExit):
    """The kit will not run on what it was given: the message says why (paths it was given, never content)."""


class Failed(RuntimeError):
    """A fixed code (`E_…`) and, at most, the kit's own argument words: never a daemon's answer."""


def live_dirs(env: dict | None = None) -> list[Path]:
    """The state directories a daemon of this machine may be serving: the default and the environment's."""
    env = os.environ if env is None else env
    out = [Path("~/.theseus").expanduser()]
    if env.get("THESEUS_STATE_DIR"):
        out.append(Path(env["THESEUS_STATE_DIR"]).expanduser())
    return [p.resolve() for p in out]


def _overlaps(a: Path, b: Path) -> bool:
    return a == b or b in a.parents or a in b.parents


def escaping_links(root: Path) -> list[Path]:
    """Every link under `root` whose target (resolved) is not inside `root`."""
    root = root.resolve()
    bad = []
    for d, dirs, files in os.walk(root, followlinks=False):
        for name in dirs + files:
            p = Path(d) / name
            if p.is_symlink():
                t = p.resolve()
                if not (t == root or root in t.parents):
                    bad.append(p)
    return bad


def check_copy(store: Path, env: dict | None = None) -> Path:
    """`store`, resolved, or `Refused`: it must be a copy no daemon serves, and lead nowhere outside itself."""
    store = Path(store).expanduser()
    if not store.is_dir():
        raise Refused(f"view kit: {store} is not a directory: give it a COPY of a store")
    store = store.resolve()
    for live in live_dirs(env):
        if _overlaps(store, live):
            raise Refused(f"view kit: {store} is a live state directory: copy it first "
                          "(cp -a, with its daemon stopped or from a backup) and give the kit the copy")
    if drive.processes_naming(store):
        raise Refused(f"view kit: a running process names {store}: a daemon serves it; give the kit a copy")
    if not (store / "store" / "wal").is_dir():
        raise Refused(f"view kit: {store} holds no store/wal: give it a copy of a daemon's state directory")
    bad = escaping_links(store)
    if bad:
        raise Refused(f"view kit: {len(bad)} link(s) in {store} lead outside it (first: {bad[0]}): "
                      "copy the store with links followed (cp -aL) and give the kit that")
    return store


def check_out(out: Path, store: Path | None = None, env: dict | None = None) -> Path:
    """`out`, resolved through its links, or `Refused`: never inside or above a live root or the input. Run it
    before anything is written."""
    out = Path(out).expanduser().resolve()
    for live in live_dirs(env):
        if _overlaps(out, live):
            raise Refused(f"view kit: the output {out} is inside or above a live state directory")
    if store is not None and _overlaps(out, Path(store).expanduser().resolve()):
        raise Refused(f"view kit: the output {out} is inside or above the store copy it reads")
    if out.exists() and any(out.iterdir()):
        raise Refused(f"view kit: {out} is not empty")
    return out


# The scratch daemon's config.
# The kernel's ledger of a STAND-IN model's calls (a local process that charges nothing): it reserves each
# call's worst case at list price before the call, so a cent would refuse the first one.
STAND_IN_LIMIT = 50.0


def config(base: dict, api_base: str = DEAD_API, model: str | None = None) -> dict:
    """The bench profile, nothing that spends, nothing that listens. `api_base` is the stand-in model's address
    when the labelled cases build a store (a local process: it charges nothing, and the kernel's limit is then
    `STAND_IN_LIMIT`, of list-price bookkeeping only)."""
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
    t.setdefault("policy", {})["agent_processes"] = []  # see the module text: its own words are the owner's
    return t


def child_env(home: Path, environ: dict | None = None) -> dict:
    """The environment of every child the kit starts: the allow-list, a home inside the scratch directory."""
    environ = os.environ if environ is None else environ
    env = {k: environ[k] for k in ENV_KEPT if k in environ}
    env.setdefault("PATH", "/usr/bin:/bin")
    env.update({"HOME": str(home), "XDG_STATE_HOME": str(home / "state"), "XDG_CONFIG_HOME": str(home / "config"),
                "XDG_CACHE_HOME": str(home / "cache"), "XDG_DATA_HOME": str(home / "data"),
                "XDG_RUNTIME_DIR": str(home), "ANTHROPIC_API_KEY": NO_KEY})
    return env


# A controlling terminal for one CLI call: a new session, the pty's far end opened and made its terminal, then
# the CLI itself in its place (the same process, so the daemon reads its terminal). No `preexec_fn`: the kit
# runs threads (the stand-in model's server).
TERMINAL = ("import fcntl, os, sys, termios; os.setsid(); fd = os.open(sys.argv[1], os.O_RDWR); "
            "fcntl.ioctl(fd, termios.TIOCSCTTY, 0); os.close(fd); os.execv(sys.argv[2], sys.argv[2:])")


def remove(path: Path) -> bool:
    """Remove `path`; on failure say so on stderr, with the path, and return False."""
    errors: list[str] = []
    shutil.rmtree(path, onerror=lambda fn, p, exc: errors.append(p))
    if Path(path).exists():
        print(f"view kit: E_REMOVE: COULD NOT REMOVE the store copy at {path}: check that no process uses it, "
              "then remove it by hand (bench/view/README.md)", file=sys.stderr)
        return False
    return True


class Scratch:
    def __init__(self, bin_dir: Path, out: Path, store: Path | None = None, api_base: str = DEAD_API,
                 model: str | None = None, env: dict | None = None):
        bins = Path(bin_dir).resolve()
        self.theseus, self.theseusd = bins / "theseus", bins / "theseusd"
        for b in (self.theseus, self.theseusd):
            if not b.is_file():
                raise SystemExit(f"view kit: no {b}")
        src = check_copy(store, env) if store is not None else None
        out = Path(out).expanduser().resolve()
        for live in live_dirs(env):
            if _overlaps(out, live):
                raise Refused(f"view kit: the output {out} is inside or above a live state directory")
        if src is not None and _overlaps(out, src):
            raise Refused(f"view kit: the output {out} is inside or above the store copy it reads")
        self.proc: subprocess.Popen | None = None
        self.dir = out / "daemon"
        self.state, self.sock, self.cfg = self.dir / "state", self.dir / "sock", self.dir / "config.toml"
        self.home = self.dir / "home"
        self.env = child_env(self.home, env)
        self._trapped, self._old_term, self._stopping, self._term_pending = False, None, False, False
        self._old_chld, self._lost = None, False
        self._trap_term()  # from before the first write until the removal has finished
        # The first line of every run: what a person removes by hand if the run is killed with SIGKILL.
        print(f"view kit: scratch directory {self.dir} (its store copy, state/, is removed at the end of the run; "
              "the directory itself stays)", file=sys.stderr, flush=True)
        made = False
        try:
            self.dir.parent.mkdir(parents=True, exist_ok=True)
            os.mkdir(self.dir, 0o700)
            made = True
            self.dir.chmod(0o700)
            self.home.mkdir(mode=0o700)
            if src is not None:
                # Links followed: none leads out (check_copy), so the copy holds plain files only.
                shutil.copytree(src, self.state, symlinks=False, ignore_dangling_symlinks=True)
            self.cfg.write_text(drive.toml_dumps(config(tomllib.loads(PROFILE.read_text()), api_base, model)))
        except BaseException:
            self._stopping = True
            if made:
                remove(self.dir)
            self._untrap()
            raise

    def cli(self, *args: str, input: str | None = None, timeout: float = 120.0,
            cwd: Path | None = None, terminal: bool = False) -> subprocess.CompletedProcess:
        """The CLI; a timeout is `Failed("E_CLI_TIMEOUT <verb>")`, never the command line it ran. `terminal`:
        run it with a controlling terminal of the kit's own, as the owner's own terminal runs it (module
        text); its input and output are still the pipes."""
        argv = [str(self.theseus), "--socket", str(self.sock), *args]
        pty = os.openpty() if terminal else None
        if pty is not None:
            argv = [sys.executable, "-I", "-c", TERMINAL, os.ttyname(pty[1]), *argv]
        try:
            return subprocess.run(argv, input=input, capture_output=True, text=True, timeout=timeout, cwd=cwd,
                                  env=self.env)
        except subprocess.TimeoutExpired:
            pass  # raised below, outside this block, so no exception holds the command line as its context
        finally:
            for fd in pty or ():
                os.close(fd)
        raise Failed(f"E_CLI_TIMEOUT {verb(args)}")

    def json(self, *args: str, **kw):
        """`theseus --json <args>`'s value, or `Failed` with a fixed code and the kit's own verb."""
        r = self.cli("--json", *args, **kw)
        if r.returncode != 0:
            raise Failed(f"E_CLI {verb(args)} exit {r.returncode}")
        try:
            return json.loads(r.stdout)
        except json.JSONDecodeError:
            raise Failed(f"E_CLI_JSON {verb(args)}") from None

    def start(self) -> "Scratch":
        try:
            self.proc = subprocess.Popen([str(self.theseusd), "--config", str(self.cfg), "--state-dir",
                                          str(self.state), "--socket", str(self.sock)],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL, env=self.env, cwd=self.home,
                                         start_new_session=True)  # its own group: the teardown kills that
            t0 = time.monotonic()
            while True:
                if self._exited():  # read without reaping: the teardown still holds the child, and its group
                    raise Failed("E_DAEMON_EXIT")
                try:
                    if self.cli("health", timeout=10).returncode == 0:
                        return self
                except Failed:  # one slow answer is not a verdict; SERVE_WAIT decides
                    pass
                if time.monotonic() - t0 > SERVE_WAIT:
                    raise Failed("E_DAEMON_TIMEOUT")
                time.sleep(0.05)
        except BaseException:
            self.stop()
            raise

    def _trap_term(self) -> None:
        def on_term(signum, frame):
            if self._stopping:  # the removal finishes first; the signal is raised after it
                self._term_pending = True
                return
            raise SystemExit(SIGTERM_EXIT)
        with contextlib.suppress(ValueError):  # not the main thread: no handler
            self._old_term = signal.signal(signal.SIGTERM, on_term)
            self._trapped = True
            # An inherited SIGCHLD-ignore makes the kernel reap the daemon the moment it exits, and its pid and
            # group id could then be reused before the teardown signals: for the kit's lifetime SIGCHLD is the
            # default, so the exited daemon stays the kit's unreaped child until the kit itself reaps it.
            if signal.getsignal(signal.SIGCHLD) == signal.SIG_IGN:
                self._old_chld = signal.signal(signal.SIGCHLD, signal.SIG_DFL)

    def _untrap(self) -> None:
        if self._trapped:
            with contextlib.suppress(ValueError, TypeError):
                signal.signal(signal.SIGTERM, self._old_term if self._old_term is not None else signal.SIG_DFL)
                if self._old_chld is not None:
                    signal.signal(signal.SIGCHLD, self._old_chld)
            self._old_chld = None
            self._trapped = False

    def _custody(self) -> str:
        """`running`, `exited` (exited, NOT reaped: still the kit's child, so its pid and its group's id cannot
        be given to another process) or `lost` (reaped by anything, `os.waitpid` elsewhere or the kernel under
        SIGCHLD-ignore included: the number may now be anyone's). Read without reaping (`WNOWAIT`). Lost is
        recorded, and is final."""
        if self._lost or self.proc is None or self.proc.returncode is not None:
            self._lost = True
            return "lost"
        try:
            r = os.waitid(os.P_PID, self.proc.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except ChildProcessError:  # ECHILD: not the kit's child any more
            self._lost = True
            return "lost"
        return "running" if r is None else "exited"

    def _exited(self) -> bool:
        return self._custody() != "running"

    def stop(self) -> list[str]:
        """The teardown, to its end whatever a step raises: shutdown (any failure or hang ignored); the wait
        for the daemon to exit, without reaping it; SIGKILL to the daemon's own process group while the kit
        still holds the unreaped child (whatever the daemon left in it goes too); the reap; the copy removed;
        the prior SIGTERM (and SIGCHLD) handlers restored. A kill or a reap that fails, or a child reaped by
        anything else (`_custody`: its group may no longer be its own, so it is never signalled), is not lost:
        once every step has been tried, it is `Failed("E_CLEANUP <scratch directory> pid <pid>")`. Then a SIGTERM held during the teardown is raised."""
        self._stopping = True
        killed: list[str] = []
        lost: int | None = None
        try:
            try:
                if self.proc is not None and not self._exited():
                    with contextlib.suppress(Exception):
                        self.cli("shutdown", timeout=SHUTDOWN_WAIT)
                    t0 = time.monotonic()
                    while not self._exited() and time.monotonic() - t0 < STOP_WAIT:
                        time.sleep(0.05)
            finally:
                if self.proc is not None:
                    pid = self.proc.pid
                    # The custody check and the signal are adjacent, and nothing of the kit reaps in between: the
                    # kit never calls poll() or wait() on the daemon before this point, it runs no reaper thread,
                    # and SIGCHLD is not ignored while it runs (`_trap_term`).
                    if self._custody() == "lost":
                        lost = pid  # never signalled: its group id is no longer the kit's to signal
                        # Marked as ended, so the dropped handle is not polled later by subprocess's own cleanup.
                        self.proc.returncode = -1
                    else:
                        try:
                            os.killpg(pid, signal.SIGKILL)  # the leader is unreaped: the group is still its own
                            killed.append(f"theseusd group {pid}")
                        except ProcessLookupError:
                            pass
                        except OSError:
                            lost = pid
                        try:
                            self.proc.wait(STOP_WAIT)
                        except (subprocess.TimeoutExpired, OSError):
                            lost = pid
                    self.proc = None
        finally:
            try:
                if self.state.exists():
                    remove(self.state)
            finally:
                self._untrap()
                self._stopping = False
        if lost is not None:
            self._term_pending = False
            raise Failed(f"E_CLEANUP {self.dir} pid {lost}")
        if self._term_pending:
            self._term_pending = False
            raise SystemExit(SIGTERM_EXIT)
        return killed

    def __enter__(self) -> "Scratch":
        return self.start()

    def __exit__(self, *exc) -> None:
        self.stop()


def verb(args) -> str:
    """The kit's own verb among a CLI call's arguments, or `cli`: never an argument's data."""
    return next((a for a in args if a in VERBS), "cli")


def ledger_rows(d: Scratch, kind: str) -> list[dict]:
    """Every row of `kind`, oldest first, paged back through `ledger.tail`'s cursor (bench/recall's
    `ledger_pages`), with fixed error codes: a page that is not what it should be is `E_LEDGER`."""
    by_pos: dict[int, dict] = {}
    before = None
    for _ in range(LEDGER_MAX // LEDGER_PAGE + 1):
        p = {"n": LEDGER_PAGE, "kind": kind, **({"before": before} if before is not None else {})}
        v = d.json("rpc", "ledger.tail", json.dumps(p))
        if isinstance(v, dict) and "result" in v and "rows" not in v:
            v = v["result"]
        rows = v.get("rows") if isinstance(v, dict) else None
        if not isinstance(rows, list):
            raise Failed("E_LEDGER")
        new = 0
        for x in rows:
            if not isinstance(x, dict) or not isinstance(x.get("position"), int):
                raise Failed("E_LEDGER")
            if x["position"] not in by_pos:
                by_pos[x["position"]] = x
                new += 1
        if not rows or not new:
            break
        nxt = v.get("older")
        before = nxt if isinstance(nxt, int) else rows[0]["position"]
    return [by_pos[k] for k in sorted(by_pos)]


def history_nodes(d: Scratch, session: str) -> list[dict]:
    """A session's nodes, oldest first (bench/recall's `history_pages`), or `E_HISTORY`."""
    pages, before = [], None
    for _ in range(10_000):
        v = d.json("history", "--full", session, *(("--before", str(before)) if before is not None else ()))
        nodes = v.get("nodes") if isinstance(v, dict) else None
        if not isinstance(nodes, list) or not all(isinstance(n, dict) for n in nodes):
            raise Failed("E_HISTORY")
        pages.append(nodes)
        if not nodes:
            break
        nxt = v.get("older") if before is not None else nodes[0].get("position")
        if not isinstance(nxt, int) or nxt == before:
            break
        before = nxt
    return [n for p in reversed(pages) for n in p]


if __name__ == "__main__":
    sys.exit("view kit: scratch.py is the kit's library and has no verbs; see bench/view/README.md")

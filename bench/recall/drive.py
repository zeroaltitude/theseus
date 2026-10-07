#!/usr/bin/env python3
"""The incidental-recall bench's drivers (theseus-7gir.17): replay one
progression to one arm, with fresh state in a scratch directory, and keep
each turn's raw answer, tokens, dollars and latency.

    python3 bench/recall/drive.py --arm theseus --memory-arm baseline \\
        --bin-dir target/release --model anthropic/claude-sonnet-5-5 \\
        --progression /tmp/rc-smoke --out /tmp/rc-th
    python3 bench/recall/drive.py --arm claude-code \\
        --model anthropic/claude-sonnet-5-5 --progression /tmp/rc-smoke --out /tmp/rc-cc

Each arm uses its own memory:

- **theseus**: a scratch `theseusd`, with the `theseus-index` beside it as
  its index tender, configured as theseus-exam's `daemon.rs` configures an
  arm's daemon: the bench profile (`bench/theseus-bench.toml`, its key
  `env:ANTHROPIC_API_KEY`), `[memory] mode = "live"` and `arm` from
  `--memory-arm` (any string: `none`, `bm25`, `baseline`, and the arms being
  built), Discord, the web UI and the MCP server off, and the index on
  except for `none`. A scratch `[catalog."<model>"] context_window` (the
  progression's, or `--context-window`) brings its compaction near the
  script's marks. A session per progression session (`sessions open`); each
  turn `theseus --json ask -s <id> -`. A CLI session has no target, so it
  is private, and recall draws on every earlier session. Compactions are
  read from the ledger (`context.compacted`) after each turn. The daemon is
  stopped at the end, and nothing is left running.
- **claude-code**: `claude -p --output-format json`, a scratch
  `CLAUDE_CONFIG_DIR`, the workspace as its working directory, a new
  session (`--session-id`) at each session boundary and `--resume <id>` for
  each next turn, and the tools that do Theseus's same work (`Bash`, `Read`,
  `Write`, `Edit`, `Glob`, `Grep`). Compaction is its own near its window:
  `--autocompact` at the progression's window when that is 100k or more,
  which is what the flag takes; below it, `/compact` through `-p --resume`
  after each mark's turn (`--cc-compact`). Compactions are read from its
  session log (`compact_boundary`). Its memory files are kept.

Every arm runs at `--effort` (default `medium`): Theseus's `profiles.bench.effort`,
Claude Code's `--effort`, Pi's `--thinking`.

Every fact's delivery is checked: its marker's text in the arm's transcript.
A probe whose fact never reached the arm is excluded by the scorer, and
counted, never scored as a miss.

Neither arm's tools are confined to the workspace on a bare host: run live
in a throwaway container or VM.

The run directory, the same for both arms (what `score.py` reads):

- `run.json`: the arm, its memory arm, the model, the progression's digest,
  the compactions (the turns whose request was compacted), Theseus's
  `compaction_rows` (each row's outcome, `compaction` or `ring`, and the
  cut's span), Theseus's `overhead` (its system prompt and tools, planned
  and measured after the first turn: past the plan by more than
  `generate.OVERHEAD_CUSHION` either way, the run stops unless
  `--allow-overhead`, which Pi's own `overhead` also reads: planned at
  `--pi-overhead`, with the compaction `threshold` set there),
  what the stop had to kill, and the totals;
- `turns.jsonl`: each turn's reply, exit, tokens, dollars and latency;
- `delivered.json`: each fact's delivery;
- `progression.json`, a copy, and `workspace/`, the arm's workspace as the
  run left it (an indirect probe's file is there);
- `raw/`: each turn's stdout, the transcripts, the daemon's log.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import tomllib
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import checks  # noqa: E402
import generate  # noqa: E402
import progression as pg  # noqa: E402
import tokens as tk  # noqa: E402

PROFILE = HERE.parent / "theseus-bench.toml"
CC_TOOLS = "Bash,Read,Write,Edit,Glob,Grep"
# The effort levels Theseus's profile, Claude Code's `--effort` and Pi's
# `--thinking` all take.
EFFORTS = ("low", "medium", "high", "xhigh", "max")
CC_AUTOCOMPACT_MIN = 100_000
STOP_WAIT = 20.0
SERVE_WAIT = 60.0
# A parent Claude Code session's variables, which would make the arm think
# it runs inside one.
CC_PARENT_ENV = ("CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT", "CLAUDE_PROJECT_DIR")


# ---- TOML, written (tomllib only reads)


def _toml_key(k: str) -> str:
    return k if k.replace("_", "").replace("-", "").isalnum() else json.dumps(k)


def _toml_value(v) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, str):
        return json.dumps(v)
    if isinstance(v, list):
        return "[" + ", ".join(_toml_value(x) for x in v) + "]"
    raise TypeError(f"no TOML for {type(v).__name__}")


def toml_dumps(t: dict) -> str:
    """Tables of scalars and lists, nested: what a daemon's config holds."""
    out: list[str] = []

    def table(prefix: list[str], d: dict) -> None:
        scalars = [(k, v) for k, v in d.items() if not isinstance(v, dict)]
        subs = [(k, v) for k, v in d.items() if isinstance(v, dict)]
        if prefix and (scalars or not subs):
            out.append("[" + ".".join(_toml_key(p) for p in prefix) + "]")
        for k, v in scalars:
            out.append(f"{_toml_key(k)} = {_toml_value(v)}")
        if scalars:
            out.append("")
        for k, v in subs:
            table(prefix + [k], v)

    table([], t)
    return "\n".join(out) + "\n"


def model_id(model: str) -> str:
    """`anthropic/claude-sonnet-5-5` → `claude-sonnet-5-5`."""
    return model.split("/", 1)[-1]


def theseus_config(
    base: dict,
    *,
    model: str,
    memory_arm: str,
    workspace: Path,
    window: int,
    spend_limit: float,
    max_loops: int,
    api_base: str | None = None,
    effort: str | None = None,
) -> dict:
    """The scratch daemon's config: the bench profile with the run's lines
    set, then daemon.rs's `config_for` (memory live at the arm; Discord, the
    web UI and the MCP server off; the index on but for `none`)."""
    t = json.loads(json.dumps(base))
    m = model_id(model)
    t.setdefault("model", {}).update({"model": m, "max_loops": max_loops})
    t.setdefault("profiles", {}).setdefault("bench", {}).update({"model": m, "max_loops": max_loops})
    if effort:
        # Sonnet 5.5's default, with none set, is high: the arms are held to
        # the one level the run names.
        t["profiles"]["bench"]["effort"] = effort
    t.setdefault("kernel", {})["spend_limit_usd"] = float(spend_limit)
    t.setdefault("tools", {})["projects_dir"] = str(workspace)
    t["memory"] = {**t.get("memory", {}), "mode": "live", "arm": memory_arm}
    t.setdefault("discord", {})["enabled"] = False
    t.setdefault("web", {})["enabled"] = False
    if "mcp_server" in t:
        t["mcp_server"]["enabled"] = False
    t.setdefault("index", {})["enabled"] = memory_arm != "none"
    # A small window wants a small output cap: the compiler keeps room for it.
    t.setdefault("catalog", {})[m] = {"context_window": int(window), "max_output_tokens": pg.output_cap(window)}
    if api_base:
        t["model"]["api_base"] = api_base
        for p in t.get("providers", {}).values():
            p["api_base"] = api_base
    return t


# ---- processes


def processes_naming(d: Path) -> list[tuple[int, str]]:
    """The processes whose command line names `d`, or whose working directory
    is in it, with their program: what a run must not leave behind
    (daemon.rs's `processes_naming`, and a job or a tool's child that
    outlived its turn)."""
    d = Path(d).resolve()
    needle = str(d).encode()
    me = os.getpid()
    out = []
    for e in Path("/proc").iterdir():
        if not e.name.isdigit() or int(e.name) == me:
            continue
        try:
            cmd = (e / "cmdline").read_bytes()
        except OSError:
            continue
        try:
            cwd = Path(os.readlink(e / "cwd"))
            inside = cwd == d or d in cwd.parents
        except OSError:
            inside = False
        if needle in cmd or inside:
            prog = Path(cmd.split(b"\0", 1)[0].decode(errors="replace")).name
            out.append((int(e.name), prog))
    return out


def run_group(cmd: list[str], text: str, timeout: float, **kw) -> tuple[str, str, int | None]:
    """`cmd` with `text` on stdin, in a process group of its own: its stdout,
    its stderr, and its exit code, or None when it ran past `timeout`, and
    then the whole group is killed (a tool's children with it)."""
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         start_new_session=True, **kw)
    try:
        out, err = p.communicate(text, timeout=timeout)
        return out, err, p.returncode
    except subprocess.TimeoutExpired:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        out, err = p.communicate()
        return out or "", (err or "") + "\ntimed out", None


def _env_without(names) -> dict:
    env = dict(os.environ)
    for n in names:
        env.pop(n, None)
    return env


def compaction_row(turn: int, rows: list[dict]) -> dict:
    """A turn's `context.compacted` rows as run.json keeps them: each one's
    outcome (`compaction`, a summary in the cut's place, or `ring`, the cut
    kept with no summary), why the ring ran instead, and the cut's span
    (its message count and its first and last positions). The scorer's
    `MOVING_OUTCOMES` says which outcomes move a probe."""
    cuts = []
    for x in rows:
        d = x.get("data") or {}
        cuts.append({k: d.get(k) for k in ("outcome", "why", "messages", "first", "last")})
    return {"turn": turn, "outcomes": [c["outcome"] for c in cuts], "cuts": cuts}


def strings(v) -> list[str]:
    """Every string in a JSON value, in order: a transcript's text."""
    out: list[str] = []
    stack = [v]
    while stack:
        x = stack.pop()
        if isinstance(x, str):
            out.append(x)
        elif isinstance(x, dict):
            stack.extend(reversed(list(x.values())))
        elif isinstance(x, list):
            stack.extend(reversed(x))
    return out


def delivery(prog: pg.Progression, transcript: str) -> dict[str, dict]:
    """Each fact's delivery: its marker in the arm's transcript (both
    normalized as the check language does)."""
    norm = checks.normalize(transcript)
    return {
        f.id: {"delivered": checks.normalize(f.marker) in norm, "marker": f.marker, "turn": f.turn}
        for f in prog.facts
    }


class OverheadPastPlan(RuntimeError):
    """The daemon's system prompt and tools differ from the progression's
    planned overhead by more than `generate.OVERHEAD_CUSHION`, over or under."""


def overhead_of(rows: list[dict], first_text: str, model: str) -> int | None:
    """The daemon's system prompt and tools, as `OVERHEAD_TOKENS` defines
    them: the earliest `context.compiled` row's estimate (a new session's
    first call), less its user message at the model's rates."""
    rows = [x for x in rows if isinstance((x.get("data") or {}).get("est_tokens"), int)]
    if not rows:
        return None
    first = min(rows, key=lambda x: x.get("position", 0))
    return first["data"]["est_tokens"] - tk.user_text(first_text).tokens(tk.rates_of(model))


def overhead_record(prog: pg.Progression, measured: int, allow: bool, planned: int | None = None) -> dict:
    """run.json's `overhead`: the planned and the measured, the cushion, and
    whether the measured is off the plan by more than it, over or under.
    `planned` is the arm's own plan where it is not the progression's (Pi's:
    `PI_OVERHEAD_TOKENS`)."""
    if planned is None:
        planned = generate.planned_overhead(prog)
    return {"planned": planned, "planned_recorded": prog.overhead_tokens is not None, "measured": measured,
            "cushion": generate.OVERHEAD_CUSHION, "past_cushion": abs(measured - planned) > generate.OVERHEAD_CUSHION,
            "allowed": allow}


def overhead_refusal(rec: dict, arm: str = "theseus") -> str:
    """The refusal, naming both numbers and how to move the plan: for Theseus
    the progression is generated again at the measured overhead, and for Pi
    its own plan moves (`--pi-overhead`: the progression is the one every
    arm reads)."""
    m, p = rec["measured"], rec["planned"]
    if arm == "pi":
        what, plan = "Pi's", "Pi's threshold was planned at"
        fix = f"Plan it at the measured overhead: run it again with --pi-overhead {m}"
    else:
        what, plan = "the daemon's", "the progression was planned at"
        fix = f"Plan it at the measured overhead: generate it again with --overhead {m}"
    way = f"past the {p:,} {plan} by {m - p:,}" if m > p else f"under the {p:,} {plan} by {p - m:,}"
    return (f"{what} system prompt and tools are {m:,} tokens, {way} (more than the cushion of "
            f"{rec['cushion']}): its marks may ring or fail to cross. {fix}, or run it as it is with --allow-overhead")


class Run:
    """The run directory and its records."""

    def __init__(self, out: Path, prog: pg.Progression, arm: str, meta: dict):
        if out.exists() and any(out.iterdir()):
            raise SystemExit(f"drive: {out} is not empty")
        out.mkdir(parents=True, exist_ok=True)
        self.out, self.prog = out, prog
        self.workspace = out / "workspace"
        self.raw = out / "raw"
        self.raw.mkdir()
        pg.materialize(prog, self.workspace)
        (out / "progression.json").write_text(json.dumps(prog.to_json(), indent=1, sort_keys=True) + "\n")
        self.meta = {"arm": arm, "digest": prog.digest(), "size": prog.size, "seed": prog.seed, **meta,
                     "started_unix": int(time.time()), "compactions": [], "killed": [], "turns": 0}
        self.turns_f = (out / "turns.jsonl").open("w")
        self.save()

    def turn(self, rec: dict) -> None:
        self.turns_f.write(json.dumps(rec, sort_keys=True) + "\n")
        self.turns_f.flush()
        self.meta["turns"] += 1

    def save(self) -> None:
        (self.out / "run.json").write_text(json.dumps(self.meta, indent=1, sort_keys=True) + "\n")

    def finish(self, transcript: str) -> None:
        self.turns_f.close()
        d = delivery(self.prog, transcript)
        (self.out / "delivered.json").write_text(json.dumps(d, indent=1, sort_keys=True) + "\n")
        self.meta["delivered"] = sum(1 for v in d.values() if v["delivered"])
        self.meta["facts"] = len(d)
        self.meta["ended_unix"] = int(time.time())
        self.save()


def _tokens(usage: dict | None) -> dict:
    u = usage or {}
    return {
        "input": int(u.get("input_tokens", 0) or 0),
        "output": int(u.get("output_tokens", 0) or 0),
        "cache_read": int(u.get("cache_read_input_tokens", 0) or 0),
        "cache_write": int(u.get("cache_creation_input_tokens", 0) or 0),
    }


# ---- Theseus


class Theseus:
    def __init__(self, a, run: Run):
        self.a, self.run = a, run
        bins = Path(a.bin_dir).resolve()
        self.theseus, self.theseusd = bins / "theseus", bins / "theseusd"
        for b in (self.theseus, self.theseusd, bins / "theseus-index"):
            if not b.is_file():
                raise SystemExit(f"drive: no {b} (theseus-index runs beside theseusd as its tender)")
        self.dir = run.out / "daemon"
        self.dir.mkdir()
        self.sock = self.dir / "sock"
        self.state = self.dir / "state"
        self.cfg = self.dir / "config.toml"
        self.proc: subprocess.Popen | None = None
        base = tomllib.loads(Path(a.profile).read_text())
        cfg = theseus_config(
            base,
            model=a.model,
            memory_arm=a.memory_arm,
            workspace=run.workspace.resolve(),
            window=a.context_window or run.prog.context_window,
            spend_limit=a.spend_limit,
            max_loops=a.max_loops,
            api_base=a.api_base,
            effort=a.effort,
        )
        self.cfg.write_text(toml_dumps(cfg))
        self.env = _env_without(("THESEUS_CONFIG", "THESEUS_STATE_DIR", "THESEUS_SOCKET", "THESEUS_SESSION"))
        if a.api_base and not self.env.get("ANTHROPIC_API_KEY"):
            self.env["ANTHROPIC_API_KEY"] = "stand-in"  # the stand-in model reads none

    def cli(self, *args: str, input: str | None = None, timeout: float = 60.0) -> subprocess.CompletedProcess:
        return subprocess.run(
            [str(self.theseus), "--socket", str(self.sock), *args],
            input=input, capture_output=True, text=True, timeout=timeout, env=self.env,
        )

    def start(self) -> None:
        with (self.run.raw / "theseusd.log").open("w") as log:
            self.proc = subprocess.Popen(
                [str(self.theseusd), "--config", str(self.cfg), "--state-dir", str(self.state), "--socket",
                 str(self.sock)],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log, env=self.env,
            )
        t0 = time.monotonic()
        while True:
            if self.proc.poll() is not None:
                raise RuntimeError(f"theseusd exited ({self.proc.returncode}) before serving: see raw/theseusd.log")
            r = self.cli("health", timeout=10)
            if r.returncode == 0:
                return
            if time.monotonic() - t0 > SERVE_WAIT:
                raise RuntimeError("theseusd did not serve in time: see raw/theseusd.log")
            time.sleep(0.05)

    def stop(self) -> list[str]:
        """`shutdown`, a kill after STOP_WAIT, then whatever names the run's
        daemon directory (its tender): what it had to kill."""
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
        while True:
            left = processes_naming(self.dir)
            if not left:
                break
            if time.monotonic() - t0 > STOP_WAIT:
                for pid, prog in left:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    killed.append(f"{prog} {pid}")
                break
            time.sleep(0.05)
        return killed

    def open_session(self, label: str) -> str:
        r = self.cli("--json", "sessions", "open", "--label", label)
        if r.returncode != 0:
            raise RuntimeError(f"sessions open failed: {r.stderr.strip()}")
        v = json.loads(r.stdout)
        sid = v.get("session_id") or v.get("id")
        if not sid:
            raise RuntimeError(f"sessions open gave no id: {r.stdout!r}")
        return sid

    def compactions(self) -> list[dict]:
        """The newest `context.compacted` rows (a page holds 1000 at most:
        far more than a run compacts)."""
        return self.ledger("context.compacted")

    def ledger(self, kind: str) -> list[dict]:
        r = self.cli("--json", "ledger", "-k", kind, "-n", "1000")
        if r.returncode != 0:
            raise RuntimeError(f"ledger failed: {r.stderr.strip()}")
        v = json.loads(r.stdout)
        rows = v.get("rows", v) if isinstance(v, dict) else v
        return [x for x in rows if isinstance(x, dict)]

    def drive(self) -> str:
        prog, run, a = self.run.prog, self.run, self.a
        self.start()
        sid, session = None, -1
        transcript: list[str] = []
        sids: list[str] = []
        run.meta["compaction_rows"] = []
        try:
            seen = {x.get("position") for x in self.compactions()}
            for t in prog.turns:
                if t.session != session:
                    session = t.session
                    sid = self.open_session(f"recall {prog.sessions[session].label}")
                    sids.append(sid)
                pg.apply_before(t, run.workspace)
                t0 = time.monotonic()
                out, err, code = run_group(
                    [str(self.theseus), "--socket", str(self.sock), "--json", "ask", "-s", sid, "-"],
                    t.text, a.turn_timeout, env=self.env)
                ms = int((time.monotonic() - t0) * 1000)
                if code is None:
                    # The turn goes on in the daemon without its client: stop
                    # it, as /stop does, so the next turn starts clean.
                    self.cli("stop", sid, timeout=60)
                (run.raw / f"t{t.index:04d}.json").write_text(out)
                try:
                    v = json.loads(out) if out.strip() else {}
                except json.JSONDecodeError:
                    v = {}
                new = [x for x in self.compactions() if x.get("position") not in seen]
                seen |= {x.get("position") for x in new}
                if new:
                    run.meta["compactions"].append(t.index)
                    run.meta.setdefault("compaction_rows", []).append(compaction_row(t.index, new))
                run.turn({
                    "index": t.index, "session": t.session, "session_id": sid, "role": t.role,
                    "reply": v.get("output", ""), "stop": v.get("stop_reason"), "exit": code,
                    "error": err.strip()[-400:] if code != 0 else None, "tokens": _tokens(v.get("usage")),
                    "cost_usd": v.get("cost_usd"), "latency_ms": ms, "compacted": bool(new),
                    "tool_calls": v.get("tool_calls"),
                })
                run.save()
                if "overhead" not in run.meta:
                    # The first turn's first compile, before any probe can move.
                    measured = overhead_of(self.ledger("context.compiled"), t.text, a.model)
                    if measured is None:
                        raise RuntimeError("no context.compiled row after the first turn: see raw/theseusd.log")
                    run.meta["overhead"] = rec = overhead_record(prog, measured, a.allow_overhead)
                    run.save()
                    if rec["past_cushion"] and not a.allow_overhead:
                        raise OverheadPastPlan(overhead_refusal(rec))
            for i, s in enumerate(sids):
                r = self.cli("--json", "history", "--full", s, timeout=120)
                (run.raw / f"history-{i + 1}.json").write_text(r.stdout)
                try:
                    transcript.extend(strings(json.loads(r.stdout)))
                except json.JSONDecodeError:
                    transcript.append(r.stdout)
                r = self.cli("--json", "memory", "recalled", s, timeout=120)
                (run.raw / f"recalled-{i + 1}.json").write_text(r.stdout or r.stderr)
        finally:
            run.meta["killed"] = self.stop()
            run.meta["left_running"] = [f"{p} {pid}" for pid, p in processes_naming(run.out)]
            run.meta["sessions"] = sids
            run.save()
        return "\n".join(transcript)


# ---- Claude Code


def cc_project_dir(config: Path, cwd: Path) -> Path:
    """Where Claude Code keeps a working directory's sessions: its path with
    every character but letters and digits made `-`."""
    name = "".join(c if c.isalnum() else "-" for c in str(cwd))
    return config / "projects" / name


class ClaudeCode:
    def __init__(self, a, run: Run):
        self.a, self.run = a, run
        self.claude = a.claude
        self.config = (run.out / "claude-config").resolve()
        self.config.mkdir()
        self.env = _env_without(CC_PARENT_ENV)
        self.env["CLAUDE_CONFIG_DIR"] = str(self.config)
        self.window = a.context_window or run.prog.context_window
        mode = a.cc_compact
        if mode == "auto":
            mode = "window" if self.window >= CC_AUTOCOMPACT_MIN else "marks"
        self.mode = mode
        run.meta["cc_compact"] = mode

    def logs(self) -> list[Path]:
        return sorted(self.config.glob("projects/*/*.jsonl"))

    def boundaries(self) -> int:
        n = 0
        for p in self.logs():
            for line in p.read_text(errors="replace").splitlines():
                if '"compact_boundary"' in line:
                    n += 1
        return n

    def call(self, text: str, sid: str, first: bool, timeout: float) -> tuple[dict, str, int | None, int]:
        cmd = [self.claude, "-p", "--output-format", "json", "--model", model_id(self.a.model),
               "--effort", self.a.effort, "--tools", CC_TOOLS, "--allowedTools", CC_TOOLS,
               "--permission-prompts", "none"]
        if self.mode == "window":
            cmd += ["--autocompact", f"{max(CC_AUTOCOMPACT_MIN, self.window) // 1000}k"]
        cmd += ["--session-id", sid] if first else ["--resume", sid]
        cmd += self.a.claude_arg
        t0 = time.monotonic()
        out, _, code = run_group(cmd, text, timeout, cwd=self.run.workspace, env=self.env)
        ms = int((time.monotonic() - t0) * 1000)
        try:
            v = json.loads(out) if out.strip() else {}
        except json.JSONDecodeError:
            v = {}
        return v, out, code, ms

    def drive(self) -> str:
        prog, run, a = self.run.prog, self.run, self.a
        sid, session, first = None, -1, True
        seen = 0
        sids: list[str] = []
        for t in prog.turns:
            if t.session != session:
                session, sid, first = t.session, str(uuid.uuid4()), True
                sids.append(sid)
            pg.apply_before(t, run.workspace)
            v, out, code, ms = self.call(t.text, sid, first, a.turn_timeout)
            (run.raw / f"t{t.index:04d}.json").write_text(out)
            if v.get("session_id"):
                sid = v["session_id"]  # carried: a resume may name it anew
                sids[-1] = sid
            first = False
            n = self.boundaries()
            compacted = n > seen
            seen = n
            if compacted:
                run.meta["compactions"].append(t.index)
            run.turn({
                "index": t.index, "session": t.session, "session_id": sid, "role": t.role,
                "reply": v.get("result", ""), "stop": v.get("subtype"), "exit": code,
                "error": None if code == 0 and not v.get("is_error") else (out[-400:] if out else "no output"),
                "tokens": _tokens(v.get("usage")), "cost_usd": v.get("total_cost_usd"), "latency_ms": ms,
                "compacted": compacted, "tool_calls": v.get("num_turns"),
            })
            if t.mark and self.mode == "marks":
                # `/compact` through print mode, after the mark's turn: its
                # boundary (if print mode ran it) lands before the next turn,
                # whose request is the compacted one.
                cv, cout, ccode, cms = self.call("/compact", sid, False, a.turn_timeout)
                (run.raw / f"t{t.index:04d}-compact.json").write_text(cout)
                run.meta.setdefault("compact_calls", []).append(
                    {"after": t.index, "exit": ccode, "ms": cms, "cost_usd": cv.get("total_cost_usd"),
                     "result": (cv.get("result") or "")[:200]})
                if cv.get("session_id"):
                    sid = cv["session_id"]
                    sids[-1] = sid
            run.save()
        transcript = []
        for p in self.logs():
            shutil.copy(p, run.raw / f"cc-{p.name}")
            for line in p.read_text(errors="replace").splitlines():
                try:
                    transcript.extend(strings(json.loads(line)))
                except json.JSONDecodeError:
                    transcript.append(line)
        # Its memory: CLAUDE.md files and the auto-memory directory, as left.
        mem = run.out / "memory"
        for p in list(run.workspace.rglob("CLAUDE.md")) + list(self.config.glob("projects/*/memory/**/*")):
            if p.is_file():
                dst = mem / p.relative_to(run.out)
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(p, dst)
        run.meta["sessions"] = sids
        run.meta["left_running"] = [f"{p} {pid}" for pid, p in processes_naming(run.out)]
        run.save()
        return "\n".join(transcript)


# ---- Pi


# The variables Pi sets for what it runs (its process markers, and its
# session's for a shell tool), which would make a nested Pi read a parent's.
PI_PARENT_ENV = ("AI_AGENT", "PI_CODING_AGENT", "PI_SESSION_ID", "PI_SESSION_FILE", "PI_PROVIDER", "PI_MODEL",
                 "PI_REASONING_LEVEL", "PI_CODING_AGENT_DIR", "PI_CODING_AGENT_SESSION_DIR")
PI_TOOLS = "read,bash,edit,write"
# A model's window in Pi's own catalog (`contextWindow`, Pi 1.0.4's
# `pi-ai` providers/data/anthropic.json: Sonnet 5.5's is 1,000,000).
PI_MODEL_WINDOW = 1_000_000
# Pi's own `keepRecentTokens` (20,000): what a compaction keeps unsummarized.
PI_KEEP_RECENT = 20_000
# Pi 1.0.4's system prompt and tools (its four tools), as `OVERHEAD_TOKENS` is
# Theseus's: the first turn's first answer's input (input, cache read and
# cache write) less the turn's words at the model's rates. Measured with Pi
# 1.0.4 itself against `standin.py`, which counts a request by the compiler's
# rule (the generator's own): 2,233 on the smoke. The provider's count of the
# same request differs from the rule's by what the rule's rates are off by,
# and the run's `overhead` shows it; `--pi-overhead` moves the plan, and a Pi
# more than `generate.OVERHEAD_CUSHION` off it, over or under, is refused.
PI_OVERHEAD_TOKENS = 2233


def pi_events(out: str) -> list[dict]:
    """Pi's `--mode json` stream: one JSON object a line."""
    events = []
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(e, dict):
                events.append(e)
    return events


def pi_turn(events: list[dict]) -> dict:
    """A Pi call's turn from its stream: the last answer's text and
    `stopReason`, its error, the tokens and dollars of its answers and of
    its compactions' summary calls summed (Pi's `usage`, with
    `cost.total`), its tool calls, and the first answer's whole input
    (`first_input`, None when it counted none): a new session's first call,
    which is the system prompt, the tools and the person's words."""
    answers = [e["message"] for e in events if e.get("type") == "message_end"
               and isinstance(e.get("message"), dict) and e["message"].get("role") == "assistant"]
    summaries = [e["result"] for e in events if e.get("type") == "compaction_end"
                 and isinstance(e.get("result"), dict) and isinstance(e["result"].get("usage"), dict)]
    toks = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
    cost, priced, tools = 0.0, bool(answers), 0
    for m in answers + summaries:
        u = m.get("usage") or {}
        for k, pk in (("input", "input"), ("output", "output"), ("cache_read", "cacheRead"),
                      ("cache_write", "cacheWrite")):
            toks[k] += int(u.get(pk) or 0)
        c = (u.get("cost") or {}).get("total")
        if c is None:
            priced = False
        else:
            cost += c
        tools += sum(1 for b in m.get("content") or [] if isinstance(b, dict) and b.get("type") == "toolCall")
    last = answers[-1] if answers else {}
    first_input = None
    if answers:
        u = answers[0].get("usage") or {}
        first_input = sum(int(u.get(k) or 0) for k in ("input", "cacheRead", "cacheWrite")) or None
    reply = "".join(b.get("text") or "" for b in last.get("content") or []
                    if isinstance(b, dict) and b.get("type") == "text")
    return {"reply": reply, "stop": last.get("stopReason"), "error": last.get("errorMessage"),
            "tokens": toks, "cost_usd": round(cost, 6) if priced else None, "tool_calls": tools,
            "answers": len(answers), "first_input": first_input}


def pi_threshold(prog: pg.Progression, window: int, pi_overhead: int) -> int:
    """The context at which Pi should compact: the progression's window less
    what Theseus's system prompt and tools hold that Pi's do not. The
    progression is planned at an overhead (`planned_overhead`) and its words
    cross the window at its marks with that overhead beside them; Pi's own
    is smaller, so its context holds the same words at the plan's less the
    difference, and that is where it crosses."""
    return window - (generate.planned_overhead(prog) - pi_overhead)


def pi_keep_recent(threshold: int) -> int:
    """What Pi's compaction keeps unsummarized at a context of `threshold`
    tokens: its own 20,000, or a quarter of a smaller threshold. A compaction
    summarizes what lies before the newest `keepRecentTokens`, and when that
    is the whole context it summarizes nothing and does not happen
    (Pi 1.0.4): at the smoke's 33k threshold, its 20k would keep nearly all
    of a context that has just passed it. It is taken from the threshold, the
    context Pi holds when it compacts, not from the window, which a plan at
    another overhead moves it from."""
    return min(PI_KEEP_RECENT, threshold // 4)


def pi_failed(code: int | None, turn: dict) -> bool:
    """A Pi turn failed when its process did not exit 0, it answered
    nothing, or its last answer ended on an error or an abort: Pi's print
    mode exits 0 when the provider fails."""
    return code != 0 or not turn["answers"] or turn["stop"] in ("error", "aborted")


class Pi:
    """Pi (`pi --print --mode json`), with a scratch `PI_CODING_AGENT_DIR` and
    session directory, the workspace as its working directory, one session
    id per progression session (`--session-id` opens it, or creates it), and
    its four tools. Its compaction is its own threshold, set where the
    progression's context crosses its window with Pi's own overhead (`pi_threshold`):
    Pi compacts when the context passes its model's window less
    `reserveTokens`, so the scratch settings set that reserve for the run's
    model to the model's window less the threshold, and `keepRecentTokens`
    to `pi_keep_recent` of the threshold. Print
    mode sends `/compact` to the model as text (Pi 1.0.4), so there are no
    marks to compact at: the threshold is the one way, at any window.
    Compactions are read from its session logs (`compaction` entries)."""

    def __init__(self, a, run: Run):
        self.a, self.run = a, run
        self.pi = a.pi
        self.config = (run.out / "pi-agent").resolve()
        self.config.mkdir()
        self.sessions = (run.out / "pi-sessions").resolve()
        self.provider, _, self.model = a.model.partition("/")
        if not self.model:
            self.provider, self.model = "anthropic", a.model
        self.window = a.context_window or run.prog.context_window
        self.threshold = pi_threshold(run.prog, self.window, a.pi_overhead)
        reserve = max(a.pi_model_window - self.threshold, 0)
        keep = pi_keep_recent(self.threshold)
        settings = {"compaction": {"modelOverrides": {f"{self.provider}/{self.model}": {
            "reserveTokens": reserve, "keepRecentTokens": keep}}}}
        (self.config / "settings.json").write_text(json.dumps(settings, indent=1) + "\n")
        if a.api_base:
            models = {"providers": {self.provider: {"baseUrl": a.api_base}}}
            (self.config / "models.json").write_text(json.dumps(models, indent=1) + "\n")
        self.env = _env_without(PI_PARENT_ENV + CC_PARENT_ENV)
        self.env.update({"PI_CODING_AGENT_DIR": str(self.config), "PI_SKIP_VERSION_CHECK": "1",
                         "PI_TELEMETRY": "0", "PI_OFFLINE": "1"})
        run.meta["pi_compact"] = {"window": self.window, "model_window": a.pi_model_window,
                                  "threshold": self.threshold, "reserve_tokens": reserve,
                                  "keep_recent_tokens": keep}

    def logs(self) -> list[Path]:
        return sorted(self.sessions.glob("*.jsonl")) if self.sessions.is_dir() else []

    def boundaries(self) -> int:
        n = 0
        for p in self.logs():
            for line in p.read_text(errors="replace").splitlines():
                if '"compaction"' not in line:
                    continue
                try:
                    n += json.loads(line).get("type") == "compaction"
                except (json.JSONDecodeError, AttributeError):
                    pass
        return n

    def call(self, text: str, sid: str, timeout: float) -> tuple[dict, str, int | None, int]:
        cmd = [self.pi, "--print", "--mode", "json", "--session-dir", str(self.sessions), "--session-id", sid,
               "--provider", self.provider, "--model", self.model, "--thinking", self.a.effort,
               "--tools", PI_TOOLS]
        cmd += self.a.pi_arg
        t0 = time.monotonic()
        out, _, code = run_group(cmd, text, timeout, cwd=self.run.workspace, env=self.env)
        ms = int((time.monotonic() - t0) * 1000)
        return pi_turn(pi_events(out)), out, code, ms

    def drive(self) -> str:
        prog, run, a = self.run.prog, self.run, self.a
        sid, session = None, -1
        seen = 0
        sids: list[str] = []
        for t in prog.turns:
            if t.session != session:
                session, sid = t.session, f"recall-{uuid.uuid4()}"
                sids.append(sid)
            pg.apply_before(t, run.workspace)
            v, out, code, ms = self.call(t.text, sid, a.turn_timeout)
            (run.raw / f"t{t.index:04d}.jsonl").write_text(out)
            n = self.boundaries()
            compacted = n > seen
            seen = n
            if compacted:
                run.meta["compactions"].append(t.index)
            failed = pi_failed(code, v)
            run.turn({
                "index": t.index, "session": t.session, "session_id": sid, "role": t.role,
                "reply": v["reply"], "stop": v["stop"], "exit": code,
                "error": (v["error"] or (out[-400:] if out else "no output")) if failed else None,
                "tokens": v["tokens"], "cost_usd": v["cost_usd"], "latency_ms": ms,
                "compacted": compacted, "tool_calls": v["tool_calls"],
            })
            run.save()
            if "overhead" not in run.meta:
                # The first turn's first answer, before any probe can move.
                if not v["first_input"]:
                    raise RuntimeError("Pi's first answer counted no input tokens: see raw/t0000.jsonl")
                measured = v["first_input"] - tk.user_text(t.text).tokens(tk.rates_of(a.model))
                rec = overhead_record(prog, measured, a.allow_overhead, a.pi_overhead)
                run.meta["overhead"] = {**rec, "threshold": self.threshold}
                run.save()
                if rec["past_cushion"] and not a.allow_overhead:
                    raise OverheadPastPlan(overhead_refusal(rec, "pi"))
        transcript = []
        for p in self.logs():
            shutil.copy(p, run.raw / f"pi-{p.name}")
            for line in p.read_text(errors="replace").splitlines():
                try:
                    transcript.extend(strings(json.loads(line)))
                except json.JSONDecodeError:
                    transcript.append(line)
        # Its memory: its compactions (in the logs, kept above) and the
        # context files it reads, as left.
        mem = run.out / "memory"
        for p in list(run.workspace.rglob("AGENTS.md")) + list(run.workspace.rglob("CLAUDE.md")):
            if p.is_file():
                dst = mem / p.relative_to(run.out)
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(p, dst)
        run.meta["sessions"] = sids
        run.meta["left_running"] = [f"{p} {pid}" for pid, p in processes_naming(run.out)]
        run.save()
        return "\n".join(transcript)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--arm", choices=("theseus", "claude-code", "pi"), required=True)
    ap.add_argument("--progression", type=Path, required=True, help="a generator's out, or its progression.json")
    ap.add_argument("--out", type=Path, required=True, help="the run directory (new or empty)")
    ap.add_argument("--model", default="anthropic/claude-sonnet-5-5")
    ap.add_argument("--effort", choices=EFFORTS, default="medium",
                    help="reasoning effort on every arm: Theseus's profile `effort`, Claude Code's `--effort`, Pi's "
                         "`--thinking`; run.json names it")
    ap.add_argument("--context-window", type=int, default=None, help="default: the progression's")
    ap.add_argument("--turn-timeout", type=float, default=900.0, help="seconds a turn may take")
    ap.add_argument("--allow-overhead", action="store_true",
                    help="run on when Theseus's or Pi's measured system prompt and tools are off the plan's overhead "
                         "(the progression's for Theseus, --pi-overhead for Pi) by more than the cushion, over or "
                         "under (both numbers are in run.json either way)")
    th = ap.add_argument_group("theseus")
    th.add_argument("--memory-arm", default="baseline", help="[memory] arm: none, bm25, baseline, or a newer arm")
    th.add_argument("--bin-dir", default="target/release", help="where theseus, theseusd and theseus-index are")
    th.add_argument("--profile", default=str(PROFILE), help="the base config (the bench profile)")
    th.add_argument("--spend-limit", type=float, default=50.0, help="the run's spend limit, in dollars")
    th.add_argument("--max-loops", type=int, default=40, help="the model calls one turn may make")
    th.add_argument("--api-base", default=None, help="a stand-in model's address (offline checks; pi's too)")
    cc = ap.add_argument_group("claude-code")
    cc.add_argument("--claude", default="claude", help="the claude binary")
    cc.add_argument("--cc-compact", choices=("auto", "window", "marks"), default="auto",
                    help="window: --autocompact at the window (100k at least); marks: /compact after each mark; "
                         "auto: window when the window is 100k or more")
    cc.add_argument("--claude-arg", action="append", default=[], help="an extra argument for claude (repeatable)")
    pa = ap.add_argument_group("pi")
    pa.add_argument("--pi", default="pi", help="the pi binary")
    pa.add_argument("--pi-model-window", type=int, default=PI_MODEL_WINDOW,
                    help="the model's window in Pi's catalog, which its reserve is taken from")
    pa.add_argument("--pi-overhead", type=int, default=PI_OVERHEAD_TOKENS,
                    help="Pi's own system prompt and tools, in tokens: its compaction is set at the progression's "
                         "window less what Theseus's overhead holds beyond it, and the run refuses a Pi measured "
                         "more than the cushion off it")
    pa.add_argument("--pi-arg", action="append", default=[], help="an extra argument for pi (repeatable)")
    a = ap.parse_args(argv)
    prog = pg.load(a.progression)
    pg.validate(prog)
    meta = {"model": a.model, "effort": a.effort, "context_window": a.context_window or prog.context_window}
    if a.arm == "theseus":
        meta["memory_arm"] = a.memory_arm
    run = Run(a.out, prog, a.arm, meta)
    driver = {"theseus": Theseus, "claude-code": ClaudeCode, "pi": Pi}[a.arm](a, run)
    try:
        transcript = driver.drive()
    except OverheadPastPlan as e:
        print(f"drive: {e}", file=sys.stderr)
        return 3
    run.finish(transcript)
    m = run.meta
    print(f"drive: {a.arm} ran {m['turns']} turns; {m['delivered']} of {m['facts']} facts delivered; "
          f"compacted at turns {m['compactions'] or 'none'}; left running: {m.get('left_running') or 'nothing'}")
    return 1 if m.get("left_running") else 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""The exam's arms: one plan, run by Theseus as configured, Theseus with a persona and card, Claude Code bare, and
Claude Code with the same persona and card in its CLAUDE.md (the fair arm).

    python3 bench/personality/exam.py run --arm theseus-persona --out /tmp/px/th-persona ...   (the CLI; see exam.py)

Each arm gets a fresh state and fresh directories, in a scratch run directory, and the plan's steps in order:

- **theseus**: the recall bench's scratch daemon (`bench/recall/drive.py`'s `Theseus`: the bench profile, `[memory]`
  live, the index on, a scratch `context_window` that brings the compactions near the plan's fills). One session
  per plan session (`sessions open`), `ask --dir <project>` for the CLI steps, the daemon's raw JSON-RPC over its
  socket for the cockpit's (`surface = "rpc"`), `ask --profile` for a routed profile's. A restart is `shutdown`
  and a start on the same state. After a step whose checks read the daemon's own record, `ledger` rows: the new
  `wake.set` rows of the session, and the `context.compiled` row's persona and file digests.
- **theseus-persona**: the same with `[context] default_persona` and `[personas.<key>] files` = the persona text
  and the card text, compiled by the daemon.
- **claude-code**: `claude -p` (the recall bench's `ClaudeCode.call`: a scratch CLAUDE_CONFIG_DIR, a session per
  plan session, `--resume` for each next turn, `/compact` after a mark), the project's directory as its working
  directory, auto-update off, the version recorded (and held to `--claude-version` when given).
- **claude-code-fair**: the same, with the persona text and the card text in the user-level
  `$CLAUDE_CONFIG_DIR/CLAUDE.md`, which Claude Code reads in every project: the same "everywhere" the daemon gives.

Results are one JSON line a step in `results.jsonl`; `run.json` holds the arm, the model, the compactions and
the totals. A run stops at its budget: the steps it did not reach are recorded `skipped`.

Standard library only.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
import tomllib
import types
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE))

import drive as rd  # noqa: E402  (bench/recall/drive.py)
import exam_discord as dc  # noqa: E402
import persona as pe  # noqa: E402
import xchecks as xc  # noqa: E402

from plan import Plan, Step  # noqa: E402

ARMS = ("theseus", "theseus-persona", "claude-code", "claude-code-fair")
RPC_AUTHOR = "cockpit:exam"


def slug(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-") or "persona"


def parse_profiles(spec: str) -> dict[str, str]:
    """`name=model,name=model` -> {name: model}."""
    out = {}
    for item in [x for x in (spec or "").split(",") if x]:
        name, _, model = item.partition("=")
        if not name or not model:
            raise ValueError(f"--profiles takes name=model[,name=model]: {item!r}")
        out[name] = model
    return out


class RunDir:
    """What the recall bench's drivers read of a `Run`: the run directory, its workspace and raw folders, and the
    context window. `workspace` is set to the project's directory before each Claude Code call."""

    def __init__(self, out: Path, window: int):
        if out.exists() and any(out.iterdir()):
            raise SystemExit(f"exam: {out} is not empty")
        self.out = out
        self.raw = out / "raw"
        self.root = out / "workspace"
        for d in (self.raw, self.root):
            d.mkdir(parents=True, exist_ok=True)
        self.workspace = self.root
        self.prog = types.SimpleNamespace(context_window=window)
        self.meta: dict = {"compactions": [], "compaction_rows": [], "killed": []}
        self.results_f = (out / "results.jsonl").open("w")

    def project(self, name: str) -> Path:
        p = self.root / name
        p.mkdir(exist_ok=True)
        return p

    def record(self, rec: dict) -> None:
        self.results_f.write(json.dumps(rec, sort_keys=True) + "\n")
        self.results_f.flush()

    def save(self) -> None:
        (self.out / "run.json").write_text(json.dumps(self.meta, indent=1, sort_keys=True) + "\n")


# ---- the daemon's JSON-RPC, as the cockpit speaks it


def rpc_call(sock_path: Path, method: str, params: dict, timeout: float) -> dict:
    """One request on the daemon's socket (NDJSON JSON-RPC 2.0); notifications before the response are read and
    dropped. Returns the `result`, or raises with the error."""
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(timeout)
    try:
        s.connect(str(sock_path))
        s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}) + "\n").encode())
        buf = b""
        while True:
            chunk = s.recv(65536)
            if not chunk:
                raise RuntimeError("the daemon closed the connection before answering")
            buf += chunk
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                try:
                    m = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if m.get("id") == 1:
                    if "error" in m:
                        raise RuntimeError(f"{method}: {m['error']}")
                    return m.get("result") or {}
    finally:
        s.close()


# ---- the runner


class Arm:
    name = ""
    has_persona = False

    def __init__(self, a, run: RunDir, plan: Plan, per: pe.Persona):
        self.a, self.run, self.plan, self.per = a, run, plan, per
        self.spent = 0.0

    # -- the loop
    def run_plan(self) -> None:
        run, plan = self.run, self.plan
        run.meta.update({"arm": self.name, "model": self.a.model, "effort": self.a.effort,
                         "plan": plan.digest(), "persona": self.per.name, "persona_digest": self.per.digest(),
                         "has_persona": self.has_persona, "window": plan.window, "budget_usd": self.a.budget,
                         "started_unix": int(time.time()), "stopped_by_budget": False})
        sessions: dict[int, object] = {}
        cur = -1
        try:
            self.start()
            for s in plan.steps:
                if self.spent > self.a.budget:
                    run.meta["stopped_by_budget"] = True
                    run.record({"index": s.index, "skipped": "budget"})
                    continue
                if s.restart_before:
                    self.restart()
                if s.session != cur:
                    cur = s.session
                    if cur not in sessions:
                        sessions[cur] = self.open(plan.sessions[cur])
                rec = self.step(s, sessions[cur])
                self.spent += rec.get("cost_usd") or 0.0
                rec["index"] = s.index
                run.record(rec)
                if rec.get("compacted"):
                    run.meta["compactions"].append(s.index)
                if s.mark:
                    self.mark(s, sessions[cur], rec)
                run.meta["steps_done"] = s.index + 1
                run.meta["spent_usd"] = round(self.spent, 6)
                run.save()
        finally:
            run.meta["killed"] = self.stop()
            run.meta["left_running"] = [f"{p} {pid}" for pid, p in rd.processes_naming(run.out)]
            run.meta["spent_usd"] = round(self.spent, 6)
            run.meta["ended_unix"] = int(time.time())
            run.results_f.close()
            run.save()

    # -- to override
    def start(self) -> None: ...
    def stop(self) -> list[str]: return []
    def restart(self) -> None: ...
    def open(self, session): raise NotImplementedError
    def step(self, s: Step, handle) -> dict: raise NotImplementedError
    def mark(self, s: Step, handle, rec: dict) -> None: ...


def turn_record(v: dict, out: str, err: str, code: int | None, ms: int, reply_key: str = "output") -> dict:
    return {"reply": v.get(reply_key, "") if reply_key in v else "", "exit": code,
            "error": None if code == 0 else (err.strip()[-400:] or out[-400:] or "no output"),
            "tokens": rd._tokens(v.get("usage")), "cost_usd": v.get("cost_usd"), "latency_ms": ms,
            "stop": v.get("stop_reason"), "tool_calls": v.get("tool_calls")}


class TheseusArm(Arm):
    name = "theseus"

    def __init__(self, a, run, plan, per):
        super().__init__(a, run, plan, per)
        a.context_window = plan.window
        self.d = rd.Theseus(a, run)
        cfg = tomllib.loads(self.d.cfg.read_text())
        key = slug(per.name)
        self.key = key
        if self.has_persona:
            pdir = self.d.dir / "persona"
            pdir.mkdir()
            (pdir / "persona.md").write_text(per.persona_text())
            (pdir / "card.md").write_text(per.card_text())
            cfg["context"] = {"default_persona": key}
            cfg["personas"] = {key: {"files": [str(pdir / "persona.md"), str(pdir / "card.md")]}}
        for pname, model in a.profile_map.items():
            cfg.setdefault("profiles", {})[pname] = {**cfg["profiles"]["bench"], "model": model}
            cfg.setdefault("catalog", {})[rd.model_id(model)] = {
                "context_window": int(plan.window), "max_output_tokens": cfg["catalog"][rd.model_id(a.model)]["max_output_tokens"]}
        self.discord: dc.FakeDiscord | None = None
        if any(s.surface == "discord" for s in plan.steps):
            sim = Path(a.bin_dir).resolve() / "theseus-sim"
            if not sim.is_file():
                raise SystemExit(f"exam: no {sim} (the Discord surface runs theseus-sim fake-discord)")
            self.discord = dc.FakeDiscord(sim, self.d.dir)
            dc.config_patch(cfg, self.discord.rest, self.discord.gateway)
            self.d.state.mkdir(parents=True, exist_ok=True)
            (self.d.state / "bindings.toml").write_text(dc.bindings_toml())
            self.d.env[dc.TOKEN_ENV] = "stand-in-token"
        self.d.cfg.write_text(rd.toml_dumps(cfg))
        self.seen_compactions: set = set()

    def start(self) -> None:
        if self.discord and self.discord.proc is None:
            self.discord.start(self.run.raw / "fake-discord.log")
        self.d.start()
        self.seen_compactions = {x.get("position") for x in self.d.compactions()}

    def stop(self) -> list[str]:
        killed = self.d.stop()
        if self.discord:
            self.discord.stop()
        return killed

    def restart(self) -> None:
        self.d.stop()
        self.d.start()

    def open(self, session) -> str:
        self.run.project(session.project)
        return self.d.open_session(f"exam {session.label}")

    def wakes(self, sid: str) -> int:
        return len([x for x in self.d.ledger("wake.set") if _session_of(x) == sid])

    def persona_digest(self, sid: str) -> bool:
        """The manifest: the session's newest `context.compiled` row names a persona and files with digests."""
        rows = [x for x in self.d.ledger("context.compiled") if _session_of(x) == sid]
        if not rows:
            return False
        data = max(rows, key=lambda x: x.get("position", 0)).get("data") or {}
        return _names_persona(data, self.key)

    def step(self, s: Step, sid: str) -> dict:
        a, d = self.a, self.d
        project = self.run.project(self.plan.sessions[s.session].project)
        ledger = xc.uses_ledger(s.checks) if s.checks else False
        before = self.wakes(sid) if ledger else 0
        t0 = time.monotonic()
        if s.surface == "discord":
            return self.discord_step(s, t0)
        if s.surface == "rpc":
            try:
                params = {"session_id": sid, "input": s.text, "author": RPC_AUTHOR}
                if s.profile:
                    params["profile"] = s.profile
                v = rpc_call(d.sock, "turn.submit", params, a.turn_timeout)
                out, err, code = json.dumps(v), "", 0
            except (RuntimeError, OSError) as e:
                v, out, err, code = {}, "", str(e), 1
        else:
            cmd = [str(d.theseus), "--socket", str(d.sock), "--json", "ask", "-s", sid, "--dir", str(project)]
            if s.profile:
                cmd += ["--profile", s.profile]
            out, err, code = rd.run_group(cmd + ["-"], s.text, a.turn_timeout, env=d.env)
            try:
                v = json.loads(out) if out.strip() else {}
            except json.JSONDecodeError:
                v = {}
        ms = int((time.monotonic() - t0) * 1000)
        if code is None:
            d.cli("stop", sid, timeout=60)
        (self.run.raw / f"s{s.index:04d}.json").write_text(out or "")
        rec = turn_record(v, out, err, code, ms)
        new = [x for x in d.compactions() if x.get("position") not in self.seen_compactions]
        self.seen_compactions |= {x.get("position") for x in new}
        rec["compacted"] = bool(new)
        if new:
            self.run.meta["compaction_rows"].append(rd.compaction_row(s.index, new))
        if ledger:
            rec["ledger"] = {"wakes_new": self.wakes(sid) - before, "persona_digest": self.persona_digest(sid)}
        return rec


    def discord_step(self, s: Step, t0: float) -> dict:
        """A message typed in the owner's DM and the reply the fake holds. The DM place has its own session; the
        daemon's turn is not seen by the CLI, so a compaction is read from the ledger as for any other step."""
        try:
            typed = self.discord.say(s.text)
            reply = self.discord.answer(typed, self.a.turn_timeout)
            err = None if reply is not None else "no reply in the DM before the timeout"
        except (RuntimeError, OSError) as e:
            reply, err = None, str(e)
        ms = int((time.monotonic() - t0) * 1000)
        new = [x for x in self.d.compactions() if x.get("position") not in self.seen_compactions]
        self.seen_compactions |= {x.get("position") for x in new}
        body, foot, cost = dc.split_footer(reply or "")
        return {"reply": body, "footer": foot, "exit": 0 if err is None else 1, "error": err, "latency_ms": ms,
                "compacted": bool(new), "cost_usd": cost, "tokens": rd._tokens(None)}


class TheseusPersonaArm(TheseusArm):
    name = "theseus-persona"
    has_persona = True


def _session_of(row: dict) -> str | None:
    return row.get("session_id") or row.get("session") or (row.get("data") or {}).get("session_id")


def _names_persona(data: dict, key: str) -> bool:
    """A `context.compiled` row names the persona in play and a digest for each of its files (context_files.rs)."""
    blob = json.dumps(data)
    return key in blob and bool(re.search(r"[0-9a-f]{8,}", blob))


class ClaudeArm(Arm):
    name = "claude-code"

    def __init__(self, a, run, plan, per):
        super().__init__(a, run, plan, per)
        a.cc_compact = "marks"
        a.context_window = plan.window
        self.cc = rd.ClaudeCode(a, run)
        self.cc.env["DISABLE_AUTOUPDATER"] = "1"
        self.cc.env["DISABLE_TELEMETRY"] = "1"
        self.cc.env["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"] = "1"
        if a.api_base:  # the exam's stand-in model
            self.cc.env["ANTHROPIC_BASE_URL"] = a.api_base
            self.cc.env.setdefault("ANTHROPIC_API_KEY", "stand-in")
        if self.has_persona:
            (self.cc.config / "CLAUDE.md").write_text(per.persona_text() + "\n" + per.card_text())
        self.version = self.check_version()
        run.meta["claude_version"] = self.version
        self.first: dict = {}
        self.sid: dict = {}

    def check_version(self) -> str:
        r = subprocess.run([self.a.claude, "--version"], capture_output=True, text=True, timeout=60, env=self.cc.env)
        v = (r.stdout or r.stderr).strip()
        if self.a.claude_version and self.a.claude_version not in v:
            raise SystemExit(f"exam: claude is {v!r}, pinned to {self.a.claude_version!r}")
        return v

    def open(self, session) -> dict:
        self.run.project(session.project)
        self.sid[session.index] = str(uuid.uuid4())
        return {"index": session.index, "project": session.project}

    def stop(self) -> list[str]:
        return []

    def call(self, text: str, h: dict, model: str | None = None) -> tuple[dict, str, int | None, int]:
        self.run.workspace = self.run.project(h["project"])
        saved = self.a.model
        if model:
            self.a.model = model
        try:
            first = h["index"] not in self.first
            v, out, code, ms = self.cc.call(text, self.sid[h["index"]], first, self.a.turn_timeout)
        finally:
            self.a.model = saved
        self.first[h["index"]] = True
        if v.get("session_id"):
            self.sid[h["index"]] = v["session_id"]
        return v, out, code, ms

    def step(self, s: Step, h: dict) -> dict:
        model = None
        if s.profile:
            model = self.a.profile_map.get(s.profile)
            if not model or not rd.model_id(model).startswith("claude-"):
                return {"skipped": f"profile {s.profile} is not a Claude model", "reply": "", "exit": None}
        v, out, code, ms = self.call(s.text, h, model)
        (self.run.raw / f"s{s.index:04d}.json").write_text(out or "")
        n = self.cc.boundaries()
        compacted = n > getattr(self, "boundaries", 0)
        self.boundaries = n
        return {"reply": v.get("result", ""), "exit": code, "tokens": rd._tokens(v.get("usage")),
                "cost_usd": v.get("total_cost_usd"), "latency_ms": ms, "compacted": compacted,
                "error": None if code == 0 and not v.get("is_error") else (out[-400:] if out else "no output"),
                "tool_calls": v.get("num_turns")}

    def mark(self, s: Step, h: dict, rec: dict) -> None:
        """`/compact` through print mode after a mark's step: its boundary lands before the next step."""
        v, out, code, ms = self.call("/compact", h)
        (self.run.raw / f"s{s.index:04d}-compact.json").write_text(out or "")
        self.spent += v.get("total_cost_usd") or 0.0
        n = self.cc.boundaries()
        if n > getattr(self, "boundaries", 0):
            self.run.meta["compactions"].append(s.index)
        self.boundaries = n
        self.run.meta.setdefault("compact_calls", []).append({"after": s.index, "exit": code, "ms": ms})


class ClaudeFairArm(ClaudeArm):
    name = "claude-code-fair"
    has_persona = True


ARM_CLASSES = {"theseus": TheseusArm, "theseus-persona": TheseusPersonaArm, "claude-code": ClaudeArm,
               "claude-code-fair": ClaudeFairArm}


def results_of(path: Path) -> dict[int, dict]:
    out = {}
    for line in Path(path).read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            out[r["index"]] = r
    return out

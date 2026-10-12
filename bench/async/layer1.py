#!/usr/bin/env python3
"""The async bench's Layer 1 drivers (theseus-2wxa): every harness on one
deterministic stand-in model, so the wall of N tool calls in one response
is the harness's alone. Standard library only.

The stand-in is `theseus-sim fake-model --rules` (crates/theseus-sim's
fake_model.rs): a Messages API on 127.0.0.1 that answers a turn whose last
user text holds a rule's `when` with that rule's tool calls, all in one
response, and any call that carries tool results with "Done.". Each harness
gets its rules in its own tool names: N calls of `sleep 2` (Claude Code's
`Bash`, Pi's and OpenCode's `bash`, Theseus's `proc_run`), N in 1, 2, 4, 8,
16. A harness that runs the N calls together takes about 2 s plus its own
overhead; one that runs them in turn, 2 s times N. The wall is measured
around the CLI, from its spawn to its exit.

    # The rules, one file for every harness, and the stand-in on them.
    python3 bench/async/layer1.py rules --out /tmp/l1/rules.json
    target/release/theseus-sim fake-model --rules /tmp/l1/rules.json --addr 127.0.0.1:9448 &

    # Each harness, 5 runs a count, its results to a JSON file.
    python3 bench/async/layer1.py run --harness claude-code --base http://127.0.0.1:9448 --out /tmp/l1/claude-code.json
    python3 bench/async/layer1.py run --harness pi          --base http://127.0.0.1:9448 --out /tmp/l1/pi.json
    python3 bench/async/layer1.py run --harness opencode    --base http://127.0.0.1:9448 --out /tmp/l1/opencode.json
    python3 bench/async/layer1.py run --harness theseus --bin target/release \\
        --base http://127.0.0.1:9448 --out /tmp/l1/theseus.json

    # What a run would do, without running it.
    python3 bench/async/layer1.py commands --harness pi --base http://127.0.0.1:9448

`--sim PATH` starts the stand-in itself (on the rules it writes) and stops
it after. Theseus's numbers of record are the turn bench's `batch` kind (the
local lane, theseus-d1hi); its row here is the same CLI-around wall as the
others', for the chart's one axis.

Each harness is pointed at the stand-in by its own base-URL setting:
- **Claude Code**: `ANTHROPIC_BASE_URL`; its side requests quieted with
  `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, and any that remain (a
  title, a quota check) answered by the stand-in's plain text, since their
  last user text holds no rule's mark, or the mark of the turn they echo;
- **Pi**: a provider of its own in `models.json` under `PI_CODING_AGENT_DIR`
  (`api: anthropic-messages`, its `baseUrl` the stand-in), as Harbor's Pi
  writes one for a configured base URL;
- **OpenCode**: `provider.anthropic.options.baseURL` in a config of its own
  (`OPENCODE_CONFIG`), as Harbor's OpenCode sets it, with the catalog fetch
  and autoupdate off, as bench/harbor's arm runs it;
- **Theseus**: `[model] api_base` in the bench profile, at the product's
  `proc_sync_secs` (60).

Whether each harness can be driven so is the first thing a run proves: a
harness whose runs never reach the stand-in, or never run the calls, is
"not measurable" with the reason (`ok: false` and its `why` in the results),
never a number.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
COUNTS = [1, 2, 4, 8, 16]
SLEEP_S = 2
MODEL = "claude-sonnet-5-5"
KEY = "layer1-stand-in"
HARNESSES = ("claude-code", "pi", "opencode", "theseus")

# Each harness's tool call for `sleep 2`: its tool's name and input.
CALL = {
    "claude-code": ("Bash", {"command": f"sleep {SLEEP_S}", "description": f"Sleep {SLEEP_S} seconds"}),
    "pi": ("bash", {"command": f"sleep {SLEEP_S}"}),
    "opencode": ("bash", {"command": f"sleep {SLEEP_S}", "description": f"Sleep {SLEEP_S} seconds"}),
    "theseus": ("proc_run", {"argv": ["sleep", str(SLEEP_S)]}),
}


def mark(harness: str, n: int) -> str:
    """The text a run's prompt holds and its rule matches: no mark is
    another's prefix (`n=01` is not `n=16`'s)."""
    return f"asyncbench-layer1 {harness} n={n:02d}"


def prompt(harness: str, n: int) -> str:
    return f"{mark(harness, n)}: run the commands you are given, then say Done."


def rules() -> list[dict[str, Any]]:
    out = []
    for h in HARNESSES:
        name, inp = CALL[h]
        for n in COUNTS:
            out.append({"when": mark(h, n), "calls": [{"name": name, "input": inp} for _ in range(n)]})
    return out


# ---------------------------------------------------------------- each harness's command


def command(harness: str, n: int, base: str, scratch: Path, bin_dir: str | None = None) -> dict[str, Any]:
    """The argv, environment, stdin and files of one run, in `scratch`."""
    text = prompt(harness, n)
    env: dict[str, str] = {"HOME": str(scratch / "home")}
    files: dict[str, str] = {}
    stdin = None
    if harness == "claude-code":
        env.update({"ANTHROPIC_BASE_URL": base, "ANTHROPIC_API_KEY": KEY, "ANTHROPIC_MODEL": MODEL,
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": MODEL, "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
                    "DISABLE_AUTOUPDATER": "1", "IS_SANDBOX": "1"})
        argv = ["claude", "--print", "--model", MODEL, "--output-format", "json", "--max-turns", "3",
                "--allowedTools", "Bash", "--dangerously-skip-permissions", text]
    elif harness == "pi":
        agent = scratch / "pi-agent"
        env["PI_CODING_AGENT_DIR"] = str(agent)
        files[str(agent / "models.json")] = json.dumps({"providers": {"layer1": {
            "baseUrl": base, "api": "anthropic-messages", "apiKey": KEY,
            "models": [{"id": MODEL, "name": MODEL, "reasoning": False, "input": ["text"],
                        "contextWindow": 200000, "maxTokens": 8192,
                        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}}]}}}, indent=2)
        argv = ["pi", "--print", "--provider", "layer1", "--model", MODEL, "--no-session", text]
    elif harness == "opencode":
        cfg = scratch / "opencode.json"
        env.update({"OPENCODE_CONFIG": str(cfg), "ANTHROPIC_API_KEY": KEY,
                    "OPENCODE_DISABLE_MODELS_FETCH": "1", "OPENCODE_DISABLE_AUTOUPDATE": "1"})
        files[str(cfg)] = json.dumps({"$schema": "https://opencode.ai/config.json",
                                      "provider": {"anthropic": {"options": {"baseURL": base, "apiKey": KEY}}},
                                      "permission": {"bash": "allow"}}, indent=2)
        argv = ["opencode", f"--model=anthropic/{MODEL}", "run", "--format=json", text]
    elif harness == "theseus":
        sys.path.insert(0, str(HERE.parent / "harbor"))
        import theseus_bench as tb

        b = str(Path(bin_dir or ".").resolve())
        cfg = scratch / "theseus.toml"
        values = tb.settings(MODEL, str(scratch / "work"), max_loops=5, spend_limit_usd=2.0, proc_sync_secs=60,
                             api_base=base)
        files[str(cfg)] = tb.profile(tb.PROFILE.read_text(), values)
        env.update({"THESEUS_CONFIG": str(cfg), "THESEUS_STATE_DIR": str(scratch / "state"),
                    "ANTHROPIC_API_KEY": KEY, "THESEUS_LOG": "warn"})
        argv = [f"{b}/theseus", "--spawn", f"{b}/theseusd", "--json", "ask", "-"]
        stdin = text
    else:
        raise ValueError(f"no harness {harness!r}: one of {', '.join(HARNESSES)}")
    return {"argv": argv, "env": env, "stdin": stdin, "files": files, "cwd": str(scratch / "work")}


# ---------------------------------------------------------------- the runs


def stand_in_up(base: str, timeout: float = 10.0) -> bool:
    """Whether the stand-in answers a Messages call."""
    deadline = time.monotonic() + timeout
    body = json.dumps({"model": MODEL, "max_tokens": 8, "stream": True,
                       "messages": [{"role": "user", "content": "ping"}]}).encode()
    while time.monotonic() < deadline:
        try:
            req = urllib.request.Request(f"{base}/v1/messages", data=body,
                                         headers={"content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=2) as r:
                if b"message_stop" in r.read():
                    return True
        except OSError:
            time.sleep(0.1)
    return False


def run_once(spec: dict[str, Any], timeout: float) -> dict[str, Any]:
    for path, text in spec["files"].items():
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        Path(path).write_text(text)
    Path(spec["cwd"]).mkdir(parents=True, exist_ok=True)
    Path(spec["env"]["HOME"]).mkdir(parents=True, exist_ok=True)
    env = {**os.environ, **spec["env"]}
    t0 = time.monotonic()
    try:
        p = subprocess.run(spec["argv"], input=spec["stdin"], env=env, cwd=spec["cwd"], capture_output=True,
                           text=True, timeout=timeout)
        code, out, err = p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        code, out, err = None, "", f"no exit in {timeout} s"
    except FileNotFoundError as e:
        return {"wall_s": None, "exit": None, "why": f"not installed: {e.filename}"}
    wall = time.monotonic() - t0
    r = {"wall_s": round(wall, 3), "exit": code}
    if code != 0:
        r["why"] = (err or out or "").strip()[-400:] or f"exit {code}"
    return r


def judge(n: int, runs: list[dict[str, Any]]) -> dict[str, Any]:
    """A count's summary: its median wall, and whether it is a number. Every
    run must exit 0 and take at least the one call's `sleep` (a run that
    never ran its calls is faster than any that did)."""
    walls = [r["wall_s"] for r in runs if r.get("exit") == 0 and r.get("wall_s") is not None]
    bad = [r for r in runs if r.get("exit") != 0]
    short = [w for w in walls if w < SLEEP_S]
    out: dict[str, Any] = {"n": n, "runs": runs, "ideal_s": SLEEP_S, "serial_s": SLEEP_S * n}
    if bad or short or not walls:
        why = bad[0].get("why") if bad else (f"a run took {min(short)} s, less than one call's {SLEEP_S} s: "
                                             "its calls never ran" if short else "no run")
        return {**out, "ok": False, "why": why, "wall_s": None}
    return {**out, "ok": True, "wall_s": round(statistics.median(walls), 3),
            "min_s": min(walls), "max_s": max(walls)}


def run(harness: str, base: str, runs: int, counts: list[int], bin_dir: str | None,
        timeout: float, keep: Path | None = None) -> dict[str, Any]:
    out: dict[str, Any] = {"schema": "asyncbench-layer1/1", "harness": harness, "base": base,
                           "sleep_s": SLEEP_S, "runs_per_count": runs, "counts": []}
    for n in counts:
        rs = []
        for i in range(runs):
            scratch = Path(tempfile.mkdtemp(prefix=f"l1-{harness}-{n}-"))
            try:
                rs.append(run_once(command(harness, n, base, scratch, bin_dir), timeout))
            finally:
                if keep:
                    # A daemon's sockets do not copy; the rest does.
                    with contextlib.suppress(shutil.Error):
                        shutil.copytree(scratch, keep / f"{harness}-n{n:02d}-{i}", dirs_exist_ok=True)
                shutil.rmtree(scratch, ignore_errors=True)
            if rs[-1].get("exit") is None and "not installed" in str(rs[-1].get("why")):
                break
        out["counts"].append(judge(n, rs))
        print(f"{harness} n={n}: " + (f"{out['counts'][-1]['wall_s']} s" if out["counts"][-1]["ok"]
                                       else f"not measurable: {out['counts'][-1]['why']}"), file=sys.stderr)
    out["ok"] = all(c["ok"] for c in out["counts"])
    if not out["ok"]:
        out["why"] = next(c["why"] for c in out["counts"] if not c["ok"])
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("rules", help="the stand-in's rules, every harness's")
    p.add_argument("--out", type=Path)
    for name in ("run", "commands"):
        p = sub.add_parser(name)
        p.add_argument("--harness", required=True, choices=HARNESSES)
        p.add_argument("--base", default="http://127.0.0.1:9448")
        p.add_argument("--bin", help="Theseus's binaries' directory (theseus, theseusd)")
        p.add_argument("--counts", default=",".join(map(str, COUNTS)))
        if name == "run":
            p.add_argument("--runs", type=int, default=5)
            p.add_argument("--timeout", type=float, default=180.0)
            p.add_argument("--out", type=Path, required=True)
            p.add_argument("--sim", help="theseus-sim, to start the stand-in on the rules (else it must run)")
            p.add_argument("--keep", type=Path, help="keep each run's scratch directory here")
    a = ap.parse_args(argv)
    if a.cmd == "rules":
        text = json.dumps(rules(), indent=1) + "\n"
        if a.out:
            a.out.parent.mkdir(parents=True, exist_ok=True)
            a.out.write_text(text)
        else:
            sys.stdout.write(text)
        return 0
    counts = [int(c) for c in a.counts.split(",")]
    if a.cmd == "commands":
        for n in counts:
            spec = command(a.harness, n, a.base, Path(f"/tmp/l1-{a.harness}-{n}"), a.bin)
            print(json.dumps({"n": n, **{k: v for k, v in spec.items() if k != "files"},
                              "files": sorted(spec["files"])}))
        return 0
    sim = None
    if a.sim:
        rules_path = Path(tempfile.mkdtemp(prefix="l1-rules-")) / "rules.json"
        rules_path.write_text(json.dumps(rules()))
        addr = a.base.split("://", 1)[-1]
        sim = subprocess.Popen([a.sim, "fake-model", "--rules", str(rules_path), "--addr", addr])
    try:
        if not stand_in_up(a.base):
            result = {"schema": "asyncbench-layer1/1", "harness": a.harness, "ok": False,
                      "why": f"the stand-in does not answer at {a.base}", "counts": []}
        else:
            result = run(a.harness, a.base, a.runs, counts, a.bin, a.timeout, a.keep)
    finally:
        if sim:
            sim.terminate()
            sim.wait(10)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(result, indent=1) + "\n")
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())

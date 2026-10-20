#!/usr/bin/env python3
"""threadbench's drivers (theseus-9ofg): a `threads-progression-v1` replayed to one arm, through the recall bench's
drivers (`bench/recall/drive.py`: Theseus, Claude Code, Pi), unchanged in what they do. Nothing here is a new
harness: it turns a threads progression into the recall bench's turns and sessions, one layout per arm, and
writes the same run directory (`run.json`, `turns.jsonl`, `delivered.json`, `workspace/`, `raw/`) plus
`threads-progression.json` and `arm.json`, which `th_score.py` reads.

The arms (A1, A1n, A1o and A2 are the threads build's, driven by `th_threads.py`; A1p has no switch to build it on):

    A1  Theseus with threads, natural addressing                    the router places each unaddressed message
    A1n A1 with the prefix library off                              what the library buys on top of threads
    A1p A1 with a prompt layer per persona                          NOT measurable: the daemon has no such layout
    A2  Theseus with threads, explicit addressing                   `ask --thread <the stream's slug>`
    A1o A1 with the ontology on (+onto)                             Jev files, a project answer attaches, recall boosts
    A3  Theseus, one session for the whole run                      measurable now
    A4  Claude Code (2.1.290, pinned), one session                  measurable now
    A5  Theseus, one session per stream: the oracle split           measurable now (a person splitting by hand)
    A6  Claude Code, `--resume` per stream                          measurable now
    A7  Pi, one session                                             measurable now
    A8  OpenCode, one session                                       only if its CLI continues a named session headless
                                                                    (`--probe-opencode` says; the report says why not)

    python3 bench/threads/th_drive.py --arm A3 --bin-dir target/release --seed 11 --size standard --out /tmp/tb-a3
    python3 bench/threads/th_drive.py --arm A5 --bin-dir target/release --progression /tmp/tb-a3 --out /tmp/tb-a5

Every arm replays the same messages in the same order. The owner's gaps are not slept (a run would take days):
they are in the progression and the scorer re-prices each arm's tokens at them (`th_price.py`). Each day's
first message opens with the date, as the recall bench's sessions do. Run live only in a throwaway container or VM.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import resource
import signal
import subprocess
import time
import sys
import types
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402,F401  (puts the recall bench on the path)
import th_generate as gen  # noqa: E402
import drive as rd  # noqa: E402  (the recall bench's drivers)
import progression as pg  # noqa: E402
import th_threads as th  # noqa: E402

REPO = HERE.parent.parent
HARBOR = HERE.parent / "harbor"
# The real Sonnet 5.5 window (theseus-core's catalog): the owner's day of work does not compact on it; whatever
# an arm does at its own window is its own.
WINDOW = 1_000_000

ARMS: dict[str, dict] = {
    "A1": {"name": "Theseus with threads, natural addressing", "harness": "theseus", "layout": "thread",
           "key": "theseus-threads", "what": "every message to the home session, unaddressed; the router places it"},
    "A1n": {"name": "Theseus with threads, no prefix library", "harness": "theseus", "layout": "thread",
            "key": "theseus-threads-noprefix", "what": "A1 with [prefix] enabled = false"},
    "A1p": {"name": "Theseus with threads, a prompt layer per persona", "harness": "theseus", "layout": "thread",
            "key": "theseus-threads-persona", "unmeasurable": th.UNMEASURABLE["A1p"],
            "what": "A1 with a prompt layer per persona instead of one persona-set layer"},
    "A2": {"name": "Theseus with threads, explicit addressing", "harness": "theseus", "layout": "thread",
           "key": "theseus-threads-addressed", "what": "each stream message sent with `ask --thread`: routing's share"},
    "A1o": {"name": "Theseus with threads and the ontology (+onto)", "harness": "theseus", "layout": "thread",
            "key": "theseus-threads-onto", "what": "A1 with [ontology] on: onto.v1 files each message, a project "
                                                  "answer attaches the thread, recall's category boost is on"},
    "A3": {"name": "Theseus, one session", "harness": "theseus", "layout": "one", "key": "theseus",
           "what": "one long session in Theseus, memory as shipped (baseline)"},
    "A4": {"name": "Claude Code, one session", "harness": "claude-code", "layout": "one", "key": "claude-code",
           "what": "one long session in Claude Code (pinned), its own compaction and memory files"},
    "A5": {"name": "Theseus, a session per stream", "harness": "theseus", "layout": "stream",
           "key": "theseus-batching", "what": "the oracle split: the owner opens a session per stream by hand"},
    "A6": {"name": "Claude Code, --resume per stream", "harness": "claude-code", "layout": "stream",
           "key": "openclaw", "what": "the oracle split in Claude Code: a session per stream, resumed by id"},
    "A7": {"name": "Pi, one session", "harness": "pi", "layout": "one", "key": "pi",
           "what": "one long session in Pi (pinned)"},
    "A8": {"name": "OpenCode, one session", "harness": "opencode", "layout": "one", "key": "other",
           "what": "one long session in OpenCode, if it can continue a named session headless"},
}
MEASURABLE = tuple(a for a, v in ARMS.items() if "unmeasurable" not in v and v["harness"] != "opencode")
REFUSAL = "arm {arm} ({name}) is not measurable: {why}"
PINS = {"claude-code": "claude_code_agent.py", "pi": "pi_agent.py", "opencode": "opencode_agent.py"}


class NotMeasurable(SystemExit):
    pass


def pinned(harness: str) -> str:
    """The version the Harbor arms pin (bench/harbor), so this bench's arms are the same builds."""
    m = re.search(r'^PINNED_VERSION\s*=\s*"([^"]+)"', (HARBOR / PINS[harness]).read_text(), re.M)
    if not m:
        raise RuntimeError(f"no PINNED_VERSION in {PINS[harness]}")
    return m.group(1)


def check_pin(harness: str, binary: str, allow: bool) -> str:
    """The binary's version, held to the pin: a run on another build is not the run the plan names."""
    want = pinned(harness)
    try:
        out = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=30).stdout
    except (OSError, subprocess.TimeoutExpired) as e:
        raise SystemExit(f"th_drive: cannot run {binary} --version: {e}")
    if want not in out and not allow:
        raise SystemExit(f"th_drive: {binary} is {out.strip() or 'unreadable'}, not the pinned {harness} {want} "
                         "(bench/harbor): install the pin, or pass --allow-version to run on this build and "
                         "say so in the report")
    return out.strip()


def check_plan(path: Path, arm: str, prog: tf.Progression) -> dict:
    """The attempt plan, committed before the run (bench/harbor/plan.py's rule): it must name the arm, the model's
    effort and this progression's digest."""
    sys.path.insert(0, str(HARBOR))
    import plan as harbor_plan
    try:
        plan = harbor_plan.load(path)
    except harbor_plan.PlanRefused as e:
        raise SystemExit(f"th_drive: {e}")
    if arm not in plan.get("arms", []):
        raise SystemExit(f"th_drive: the plan {plan['file']} does not name arm {arm} (it names {plan.get('arms')})")
    digests = {v["digest"] for v in plan.get("progressions", {}).values()}
    if prog.digest() not in digests:
        raise SystemExit(f"th_drive: this progression ({prog.digest()[:12]}) is not one the plan {plan['file']} "
                         "pins: generate it again, or change the plan and commit it first")
    return {k: plan[k] for k in ("file", "sha256", "commit", "head", "name")}


def stop_stand_in_services(run) -> list[str]:
    """The local-ops stream asks its arm to restart a stand-in service, which stays up in the workspace when the work
    is done. The recall drivers fail a run that leaves a process working in it, so the service is stopped here, by
    the pids found in the run's own workspace (never by name), and named in run.json. What is still running after
    this is a real leftover, and the exit code says so."""
    killed = []
    for pid, prog in rd.processes_naming(run.workspace):
        try:
            os.kill(pid, signal.SIGKILL)
            killed.append(f"{prog} {pid}")
        except ProcessLookupError:
            pass
    t0 = time.monotonic()
    while rd.processes_naming(run.workspace) and time.monotonic() - t0 < 5:
        time.sleep(0.05)
    return killed


def refuse(arm: str) -> None:
    raise NotMeasurable(REFUSAL.format(arm=arm, name=ARMS[arm]["name"], why=ARMS[arm]["unmeasurable"]))


def probe_opencode(binary: str = "opencode") -> dict:
    """Can OpenCode continue a named session headless? It can if `run --help` offers a way to name or continue one
    (`--session`, `--continue`), and then the A8 arm is worth a driver; if it cannot be run at all, or offers
    none, the report says "not measurable" and why."""
    try:
        r = subprocess.run([binary, "run", "--help"], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"measurable": False, "why": f"opencode could not be run here ({e.__class__.__name__}: {e})"}
    text = r.stdout + r.stderr
    flags = [f for f in ("--session", "--continue", "-c", "-s") if re.search(rf"(?<![\w-]){re.escape(f)}(?![\w-])", text)]
    if r.returncode != 0 and not flags:
        return {"measurable": False, "why": f"`opencode run --help` exited {r.returncode} and listed no session flag"}
    named = any(f in flags for f in ("--session", "-s"))
    if not named:
        return {"measurable": False, "flags": flags,
                "why": "`opencode run` offers no way to name a session headless, so one session cannot be "
                       "continued across the run's messages (a `--continue` of the last session alone cannot "
                       "carry a per-stream layout, and the one-session layout needs the same id each turn)"}
    return {"measurable": True, "flags": flags,
            "why": "`opencode run` can name a session; a driver can continue it, not built in T0 (the probe's "
                   "answer is the finding)"}


# ---------------------------------------------------------------- the recall bench's view of a threads run


def session_key(prog: tf.Progression, layout: str, m: tf.Msg, state: dict) -> str:
    """Which session a message goes to under `layout`: `one` (all in one; a prefix track's cold gap starts a fresh
    one), `stream` (one per stream; the prefix track's reopen starts a fresh one for that stream)."""
    if layout == "one":
        if prog.track == "prefix" and m.reopen and state.get("reopened") != m.day:
            state["epoch"] = state.get("epoch", 0) + 1  # one cold gap, one fresh session for the whole run
            state["reopened"] = m.day
        return f"all-{state.get('epoch', 0)}"
    if m.reopen:
        state[m.stream] = state.get(m.stream, 0) + 1
    return f"{m.stream}-{state.get(m.stream, 0)}"


def view(prog: tf.Progression, layout: str, overhead: int | None = None, window: int = WINDOW) -> pg.Progression:
    """The recall bench's progression for a threads one: its turns (a message each), its sessions (the layout's),
    its facts (their markers, for the delivery check) and the workspace. Probes are scored by th_score.py, so the
    view has none."""
    state: dict = {}
    keys: list[str] = []
    sessions: list[pg.Session] = []
    turns: list[pg.Turn] = []
    start = __import__("datetime").date.fromisoformat(prog.notes["first_day"])
    prev_day = -1
    for m in prog.messages:
        k = session_key(prog, layout, m, state)
        if k not in keys:
            keys.append(k)
            d = start + __import__("datetime").timedelta(days=m.day)
            sessions.append(pg.Session(label=f"threads {k}", date=d.isoformat(), weekday=d.strftime("%A")))
        text = m.text
        if m.day != prev_day or m.reopen:
            d = start + __import__("datetime").timedelta(days=m.day)
            text = f"It's {d.strftime('%A')}, {d.isoformat()}. {text}"
        prev_day = m.day
        turns.append(pg.Turn(index=m.index, session=keys.index(k), block=[s.id for s in prog.streams].index(m.stream),
                             topic=m.stream, text=text, role="probe" if m.role == "probe" else "filler",
                             est_tokens=0, before=m.before))
    facts = [pg.Fact(id=f.id, subject=f.subject, kind=f.kind, value=f.value, salience="central", family="said",
                     carrier="topic", turn=f.msg, source="said", marker=f.marker) for f in prog.facts]
    return pg.Progression(format=pg.FORMAT, seed=prog.seed, size=prog.size, sessions=sessions, turns=turns,
                          facts=facts, probes=[], workspace=prog.workspace, context_window=window,
                          overhead_tokens=overhead)


# ---------------------------------------------------------------- the arguments the recall drivers read


def namespace(a: argparse.Namespace) -> types.SimpleNamespace:
    """The attributes the recall bench's driver classes read, from this CLI's."""
    return types.SimpleNamespace(
        model=a.model, effort=a.effort, context_window=a.context_window or WINDOW, turn_timeout=a.turn_timeout,
        allow_overhead=a.allow_overhead, memory_arm=a.memory_arm, bin_dir=a.bin_dir, profile=a.profile,
        spend_limit=a.spend_limit, day_ceiling=a.day_ceiling, max_loops=a.max_loops, api_base=a.api_base,
        claude=a.claude, cc_compact="marks", claude_arg=a.claude_arg, pi=a.pi, pi_model_window=a.pi_model_window,
        pi_overhead=a.pi_overhead, pi_arg=a.pi_arg)


def parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--arm", choices=tuple(ARMS), default=None)
    ap.add_argument("--probe-opencode", action="store_true", help="say whether A8 is measurable, and why not")
    ap.add_argument("--opencode", default="opencode")
    ap.add_argument("--progression", type=Path, default=None, help="a generator's out or a run directory")
    ap.add_argument("--seed", type=int, default=gen.DEV_SEED)
    ap.add_argument("--size", choices=tuple(gen.PLAN), default="standard")
    ap.add_argument("--out", type=Path, default=None, help="the run directory (new or empty)")
    ap.add_argument("--model", default="anthropic/claude-sonnet-5-5")
    ap.add_argument("--effort", choices=rd.EFFORTS, default="medium")
    ap.add_argument("--context-window", type=int, default=None, help=f"default {WINDOW:,}, Sonnet 5.5's own")
    ap.add_argument("--turn-timeout", type=float, default=900.0)
    ap.add_argument("--allow-overhead", action="store_true")
    ap.add_argument("--allow-version", action="store_true", help="run a harness that is not the pinned build")
    ap.add_argument("--plan", type=Path, default=None,
                    help="the attempt plan (bench/plans/threadbench-t0r.json): refuse unless it is committed and clean "
                         "and names this arm and this progression")
    tg = ap.add_argument_group("theseus")
    tg.add_argument("--memory-arm", default="baseline")
    tg.add_argument("--bin-dir", default="target/release")
    tg.add_argument("--profile", default=str(rd.PROFILE))
    tg.add_argument("--spend-limit", type=float, default=75.0, help="the run's spend limit, in dollars")
    tg.add_argument("--day-ceiling", type=float, default=None)
    tg.add_argument("--max-loops", type=int, default=40)
    tg.add_argument("--api-base", default=None)
    tg.add_argument("--config-overlay", type=Path, default=None,
                    help="threads arms: a TOML laid over the scratch config (Jev's endpoint, for one); its sha256 is "
                         "kept in run.json")
    cc = ap.add_argument_group("claude-code")
    cc.add_argument("--claude", default="claude")
    cc.add_argument("--claude-arg", action="append", default=[])
    pa = ap.add_argument_group("pi")
    pa.add_argument("--pi", default="pi")
    pa.add_argument("--pi-model-window", type=int, default=rd.PI_MODEL_WINDOW)
    pa.add_argument("--pi-overhead", type=int, default=None)
    pa.add_argument("--pi-arg", action="append", default=[])
    pa.add_argument("--theseus-overhead", type=int, default=None,
                    help="Pi: the overhead the plan holds (a Theseus run's run.json `overhead.planned`)")
    return ap


def main(argv: list[str] | None = None) -> int:
    ap = parser()
    a = ap.parse_args(argv)
    if a.probe_opencode:
        print(json.dumps(probe_opencode(a.opencode), indent=1))
        return 0
    if not a.arm or not a.out:
        ap.error("--arm and --out are required")
    spec = ARMS[a.arm]
    if "unmeasurable" in spec:
        refuse(a.arm)
    if spec["harness"] == "opencode":
        p = probe_opencode(a.opencode)
        raise NotMeasurable(f"arm A8 (OpenCode) is not measurable here: {p['why']}")
    if a.out.exists() and any(a.out.iterdir()):
        raise SystemExit(f"th_drive: {a.out} is not empty")
    if a.progression is not None:
        prog = tf.load(a.progression)
    else:
        prog = gen.build(a.seed, a.size)
    tf.validate(prog)
    plan_rec = check_plan(a.plan, a.arm, prog) if a.plan else None
    harness = spec["harness"]
    if harness in ("claude-code", "pi"):
        check_pin(harness, a.claude if harness == "claude-code" else a.pi, a.allow_version)
    ns = namespace(a)
    overhead = None
    probe = None
    threaded = spec["layout"] == "thread"
    if harness == "theseus" and not threaded:
        probe = rd.measure_theseus(ns)
        overhead = probe["measured"]
    elif harness == "pi":
        overhead = a.theseus_overhead
        if overhead is None:
            raise SystemExit("th_drive: Pi plans its compaction from the overhead the Theseus arm measured: pass "
                             "--theseus-overhead N (a Theseus run's run.json, overhead.planned)")
    v = view(prog, spec["layout"], overhead)
    meta = {"plan": plan_rec, "model": a.model, "effort": a.effort, "context_window": v.context_window, "arm_id": a.arm,
            "arm_name": spec["name"], "layout": spec["layout"], "harness": harness, "sessions_planned": len(v.sessions)}
    if harness == "theseus":
        meta["memory_arm"] = a.memory_arm
    overlay = None
    if threaded and a.config_overlay:
        raw = a.config_overlay.read_bytes()
        overlay = __import__("tomllib").loads(raw.decode())
        meta["config_overlay"] = {"path": str(a.config_overlay), "sha256": __import__("hashlib").sha256(raw).hexdigest()}
    run = rd.Run(a.out, v, harness, meta)
    prog.save(a.out)
    (a.out / "arm.json").write_text(json.dumps({"arm": a.arm, "name": spec["name"], "layout": spec["layout"],
                                                "harness": harness, "key": spec["key"],
                                                "progression_digest": prog.digest()}, indent=1) + "\n")
    if probe is not None:
        (run.raw / "probe-theseusd.log").write_text(probe["log"])
        run.meta["overhead"] = rec = {**rd.overhead_record(overhead, probe["measured"], a.allow_overhead, False),
                                      "source": "probe", "probe_cost_usd": probe["cost_usd"]}
        run.save()
    if threaded:
        # The probe's plain session carries neither the persona layer nor a thread's card: no overhead is claimed.
        run.meta["overhead"] = {"source": "not measured: a threads arm's prompt is not the probe's"}
        run.save()
        driver = th.ThreadsTheseus(ns, run, a.arm, prog, overlay)
    else:
        driver = {"theseus": rd.Theseus, "claude-code": rd.ClaudeCode, "pi": rd.Pi}[harness](ns, run)
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    try:
        transcript = driver.drive()
    except rd.OverheadPastPlan as e:
        print(f"th_drive: {e}", file=sys.stderr)
        run.turns_f.close()
        return 3
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    stopped = stop_stand_in_services(run)
    # The driven processes' own work (the harness's CLI, and a Theseus daemon, which is a child), apart from the
    # model's: CPU seconds, and the largest peak RSS of any one of them.
    run.meta["resources"] = {"cpu_user_s": round(after.ru_utime - before.ru_utime, 2),
                             "cpu_sys_s": round(after.ru_stime - before.ru_stime, 2), "max_rss_kb": after.ru_maxrss}
    run.meta["stand_in_services_stopped"] = stopped
    run.meta["left_running"] = [f"{p} {pid}" for pid, p in rd.processes_naming(run.out)]
    run.finish(transcript)
    m = run.meta
    print(f"th_drive: {a.arm} ({spec['name']}) ran {m['turns']} turns in {len(m.get('sessions', []))} session(s); "
          f"{m['delivered']} of {m['facts']} facts delivered; left running: {m.get('left_running') or 'nothing'}")
    return 1 if m.get("left_running") else 0


if __name__ == "__main__":
    sys.exit(main())

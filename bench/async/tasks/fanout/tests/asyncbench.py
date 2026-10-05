#!/usr/bin/env python3
"""The async bench's tools, ledger, and checks (theseus-7gir.16).

One file, standard library only, copied into every task: its image's
`/opt/async/lib/asyncbench.py` (the tools the agent runs, from
`/opt/async/bin`) and its `tests/asyncbench.py` (the verifier's own copy, so
an agent that edits the image's cannot change the check). `sync.py` writes
the copies, and a test holds them to this file.

    asyncbench.py tool NAME ARGS...   a family's tool (the bin wrappers)
    asyncbench.py check FAMILY        the verifier: reward.json, the ledger copied
    asyncbench.py await --tool T --kind K --timeout S [--count N]
                                      block until the ledger has N such records
    asyncbench.py inject --message M --trigger JSON
                                      the driver's injection, as a ledger record

Paths come from `ASYNC_ROOT` (default `/`): the task's files under
`$ASYNC_ROOT/app`, the ledger and the tools' state under
`$ASYNC_ROOT/var/lib/async` (outside the working directory), and the
verifier's logs under `$ASYNC_ROOT/logs/verifier`. `ASYNC_TIME_SCALE`
(default 1) shortens every step's sleep, for tests; each start records it, and
the check accepts only the scale its own environment names, so an agent that
sets it fails.

The ledger is JSONL, one record per line, appended under an exclusive lock:
`seq`, `kind` (start, end, fail, effect, stopped, violation, inject), `tool`,
`step`, `pid`, `ppid`, `pstart` (the process's start, from /proc, so a reused
pid is not mistaken for it), `wall` and `mono` (time.time and
time.monotonic, read under the lock, so both only grow), the record's own
fields, and `prev` and `hash`: each hash is the sha256 of the previous hash
and the record, so an edited, dropped, or reordered line breaks the chain.
"""

from __future__ import annotations

import contextlib
import fcntl
import hashlib
import json
import os
import random
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterator

GENESIS = "0" * 64
_rng = random.SystemRandom()


# ---------------------------------------------------------------- paths


def root() -> Path:
    return Path(os.environ.get("ASYNC_ROOT") or "/")


def app() -> Path:
    return root() / "app"


def var() -> Path:
    return root() / "var/lib/async"


def ledger_path() -> Path:
    return var() / "ledger.jsonl"


def state() -> Path:
    return var() / "state"


def verifier_logs() -> Path:
    return root() / "logs/verifier"


def scale() -> float:
    try:
        return float(os.environ.get("ASYNC_TIME_SCALE") or "1")
    except ValueError:
        return 1.0


# ---------------------------------------------------------------- ledger


def _canonical(rec: dict[str, Any]) -> str:
    return json.dumps(rec, sort_keys=True, separators=(",", ":"))


def chain_hash(prev: str, rec: dict[str, Any]) -> str:
    body = {k: v for k, v in rec.items() if k != "hash"}
    return hashlib.sha256((prev + _canonical(body)).encode()).hexdigest()


def pstart(pid: int) -> str | None:
    """The process's start time in clock ticks since boot (/proc/PID/stat's
    22nd field), or None where there is no /proc."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    return stat[stat.rindex(")") + 2 :].split()[19]


@contextlib.contextmanager
def _locked(path: Path) -> Iterator[Any]:
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a+") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        try:
            yield f
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def append(kind: str, tool: str = "", step: str = "", **fields: Any) -> dict[str, Any]:
    """Append one record; its seq, times, and hash are read under the lock.
    SIGTERM waits while it is held: the handler appends too, and a second
    lock of the file by its own process would wait for ever."""
    old = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
    try:
        return _append(kind, tool, step, fields)
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, old)


def _append(kind: str, tool: str, step: str, fields: dict[str, Any]) -> dict[str, Any]:
    with _locked(ledger_path()) as f:
        f.seek(0)
        lines = [line for line in f.read().splitlines() if line.strip()]
        prev, seq = GENESIS, 0
        if lines:
            last = json.loads(lines[-1])
            prev, seq = last["hash"], last["seq"] + 1
        pid = os.getpid()
        rec: dict[str, Any] = {
            "seq": seq,
            "kind": kind,
            "tool": tool,
            "step": step,
            "pid": pid,
            "ppid": os.getppid(),
            "pstart": pstart(pid),
            "wall": time.time(),
            "mono": time.monotonic(),
            **fields,
            "prev": prev,
        }
        rec["hash"] = chain_hash(prev, rec)
        f.seek(0, os.SEEK_END)
        f.write(_canonical(rec) + "\n")
        f.flush()
        os.fsync(f.fileno())
        return rec


def read(path: Path | None = None) -> list[dict[str, Any]]:
    p = path or ledger_path()
    try:
        text = p.read_text()
    except FileNotFoundError:
        return []
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def verify_chain(records: list[dict[str, Any]]) -> list[str]:
    """What is wrong with the ledger as a record of itself: a broken chain,
    a gap in seq, or a time that went back."""
    problems, prev = [], GENESIS
    last_mono = last_wall = float("-inf")
    for i, r in enumerate(records):
        if r.get("seq") != i:
            problems.append(f"line {i}: seq {r.get('seq')} is not {i}")
        if r.get("prev") != prev or r.get("hash") != chain_hash(prev, r):
            problems.append(f"line {i}: its hash does not follow the line before it")
        if r.get("mono", 0) < last_mono or r.get("wall", 0) < last_wall - 1:
            problems.append(f"line {i}: its time goes back")
        prev = r.get("hash", "")
        last_mono, last_wall = r.get("mono", 0), r.get("wall", 0)
    return problems


def verify_steps(records: list[dict[str, Any]], want_scale: float) -> list[str]:
    """Each end or fail follows a start of the same step by the same process,
    at least the start's drawn duration later; each start was drawn at the
    check's own time scale."""
    problems = []
    starts: dict[tuple[str, int], dict[str, Any]] = {}
    for r in records:
        key = (r.get("step", ""), r.get("pid", -1))
        if r["kind"] == "start":
            if abs(float(r.get("scale", -1)) - want_scale) > 1e-9:
                problems.append(f"{r['step']}: ran at time scale {r.get('scale')}, not {want_scale}")
            starts[key] = r
        elif r["kind"] in ("end", "fail"):
            s = starts.get(key)
            if s is None:
                problems.append(f"{r['step']}: an {r['kind']} with no start by pid {r['pid']}")
                continue
            took = r["mono"] - s["mono"]
            need = float(s.get("sleep", 0)) * (0.5 if r["kind"] == "fail" else 1.0)
            if took + 0.05 < need:
                problems.append(f"{r['step']}: ended {took:.2f}s after its start, before its {need:.2f}s")
    return problems


def by(records: list[dict[str, Any]], kind: str, tool: str | None = None) -> list[dict[str, Any]]:
    return [r for r in records if r["kind"] == kind and (tool is None or r["tool"] == tool)]


# ---------------------------------------------------------------- steps


class Step:
    """One slow step of a tool: its duration drawn at its start and logged;
    a SIGTERM logs `stopped` and exits 143."""

    def __init__(self, tool: str, step: str, lo: float, hi: float, **fields: Any):
        self.tool, self.step = tool, step
        self.children: list[subprocess.Popen] = []
        self.stopping = False
        self.duration = round(_rng.uniform(lo, hi), 3)
        self.sleep = self.duration * scale()
        signal.signal(signal.SIGTERM, self._stopped)
        self.start = append("start", tool, step, duration=self.duration, scale=scale(),
                            sleep=self.sleep, **fields)

    def _stopped(self, *_: Any) -> None:
        # A stop passes to the step's children, and waits for them, as a
        # well-behaved parent's does; a second SIGTERM meanwhile is the same stop.
        if self.stopping:
            return
        self.stopping = True
        for c in self.children:
            with contextlib.suppress(ProcessLookupError):
                c.terminate()
        for c in self.children:
            c.wait()
        append("stopped", self.tool, self.step, signal="SIGTERM")
        sys.exit(143)

    def wait(self, fraction: float = 1.0) -> None:
        end = self.start["mono"] + self.sleep * fraction
        while (left := end - time.monotonic()) > 0:
            time.sleep(left)

    def end(self, **fields: Any) -> dict[str, Any]:
        return append("end", self.tool, self.step, **fields)


@contextlib.contextmanager
def _state(name: str, default: Any) -> Iterator[dict[str, Any]]:
    """A tool's state file, read and written under its lock."""
    path = state() / name
    with _locked(path.with_suffix(".lock")):
        try:
            data = json.loads(path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            data = default() if callable(default) else default
        box = {"data": data}
        yield box
        tmp = path.with_suffix(".tmp")
        tmp.write_text(json.dumps(box["data"], sort_keys=True))
        tmp.replace(path)


def _alive(pid: int, started: str | None) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass
    now = pstart(pid)
    if now is None:
        return True
    try:
        # A zombie has exited: only its parent's wait remains.
        if Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "Z":
            return False
    except OSError:
        return False
    return started is None or now == started


# ---------------------------------------------------------------- the families' tools

PARTS = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"]
INGEST = ["1", "2", "3", "4", "5", "6"]
ACCOUNTS = {"acct-north": 100, "acct-south": 100, "acct-east": 100, "acct-west": 100}
DEPOSITS_AT_ONCE = 2
MIGRATE_WORKERS = 2

# Each family's step durations, in seconds before the time scale.
DURATIONS = {
    "digest": (20.0, 45.0),
    "build-index": (60.0, 180.0),
    "train-model": (150.0, 210.0),
    "ticket-count": (1.0, 3.0),
    "ingest": (15.0, 30.0),
    "migrate": (900.0, 900.0),
    "deposit": (8.0, 15.0),
}


def _usage(msg: str) -> int:
    print(msg, file=sys.stderr)
    return 2


def tool_digest(args: list[str]) -> int:
    if len(args) != 1 or args[0] not in PARTS:
        return _usage(f"usage: digest NAME, NAME one of {', '.join(PARTS)}")
    s = Step("digest", args[0], *DURATIONS["digest"])
    s.wait()
    value = f"{_rng.getrandbits(32):08x}"
    s.end(value=value)
    print(f"{args[0]}: {value}")
    return 0


def tool_build_index(args: list[str]) -> int:
    if args:
        return _usage("usage: build-index")
    s = Step("build-index", "index", *DURATIONS["build-index"])
    s.wait()
    token = f"idx-{_rng.getrandbits(40):010x}"
    s.end(token=token)
    print(f"index built: {token}")
    return 0


def tool_train_model(args: list[str]) -> int:
    if args:
        return _usage("usage: train-model")
    s = Step("train-model", "train", *DURATIONS["train-model"])
    for epoch in range(1, 6):
        s.wait(epoch / 5)
        print(f"epoch {epoch}/5", flush=True)
    score = _rng.randint(7000, 9999)
    s.end(score=score)
    print(f"final score: {score}")
    return 0


def tool_ticket_count(args: list[str]) -> int:
    if args:
        return _usage("usage: ticket-count")
    s = Step("ticket-count", "tickets", *DURATIONS["ticket-count"])
    s.wait()
    n = _rng.randint(100, 999)
    s.end(count=n)
    print(f"open tickets: {n}")
    return 0


def tool_ingest(args: list[str]) -> int:
    if len(args) != 1 or args[0] not in INGEST:
        return _usage(f"usage: ingest PART, PART one of {' '.join(INGEST)}")
    part = args[0]
    with _state("ingest.json", lambda: {"flaky": sorted(_rng.sample(INGEST, 2)), "tries": {}}) as st:
        d = st["data"]
        tries = d["tries"].get(part, 0)
        d["tries"][part] = tries + 1
        fails = part in d["flaky"] and tries == 0
    s = Step("ingest", part, *DURATIONS["ingest"], attempt=tries + 1)
    if fails:
        s.wait(0.5)
        append("fail", "ingest", part, error="503 upstream unavailable")
        print(f"ingest {part}: transient error: 503 upstream unavailable; try again", file=sys.stderr)
        return 75
    s.wait()
    rows = _rng.randint(100, 999)
    s.end(rows=rows)
    append("effect", "ingest", part, rows=rows)
    print(f"part {part}: {rows} rows")
    return 0


def tool_migrate(args: list[str]) -> int:
    if args:
        return _usage("usage: migrate")
    s = Step("migrate", "migrate", *DURATIONS["migrate"], role="main")
    for i in range(1, MIGRATE_WORKERS + 1):
        s.children.append(subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), "tool", "migrate-worker", str(i)]))
    print("migrating: 2 workers started", flush=True)
    s.wait()
    for w in s.children:
        w.wait()
    s.end()
    append("effect", "migrate", "migrate", migrated=True)
    print("migration done")
    return 0


def tool_migrate_worker(args: list[str]) -> int:
    s = Step("migrate", f"worker-{args[0]}", *DURATIONS["migrate"], role="worker")
    s.wait()
    s.end()
    return 0


def tool_deposit(args: list[str]) -> int:
    if len(args) != 2 or args[0] not in ACCOUNTS or not args[1].isdigit():
        return _usage(f"usage: deposit ACCOUNT AMOUNT, ACCOUNT one of {' '.join(ACCOUNTS)}")
    account, amount = args[0], int(args[1])
    me = os.getpid()
    with _state("service.json", lambda: {"active": []}) as st:
        active = [a for a in st["data"]["active"] if _alive(a[0], a[1])]
        if len(active) >= DEPOSITS_AT_ONCE:
            st["data"]["active"] = active
            refused = True
        else:
            st["data"]["active"] = active + [[me, pstart(me)]]
            refused = False
    if refused:
        append("violation", "deposit", account, why=f"more than {DEPOSITS_AT_ONCE} deposits at once",
               amount=amount)
        print(f"deposit: 429 too many requests: the service takes at most {DEPOSITS_AT_ONCE} "
              "deposits at once", file=sys.stderr)
        return 3
    try:
        with _state("balances.json", dict(ACCOUNTS)) as st:
            before = st["data"][account]
        s = Step("deposit", account, *DURATIONS["deposit"], amount=amount, read=before)
        s.wait()
        with _state("balances.json", dict(ACCOUNTS)) as st:
            st["data"][account] = before + amount
        s.end(amount=amount, wrote=before + amount)
        append("effect", "deposit", account, amount=amount)
        print(f"{account}: deposited {amount}")
        return 0
    finally:
        with _state("service.json", lambda: {"active": []}) as st:
            st["data"]["active"] = [a for a in st["data"]["active"] if a[0] != me]


def tool_balance(args: list[str]) -> int:
    if len(args) != 1 or args[0] not in ACCOUNTS:
        return _usage(f"usage: balance ACCOUNT, ACCOUNT one of {' '.join(ACCOUNTS)}")
    with _state("balances.json", dict(ACCOUNTS)) as st:
        print(st["data"][args[0]])
    return 0


TOOLS = {
    "digest": tool_digest,
    "build-index": tool_build_index,
    "train-model": tool_train_model,
    "ticket-count": tool_ticket_count,
    "ingest": tool_ingest,
    "migrate": tool_migrate,
    "migrate-worker": tool_migrate_worker,
    "deposit": tool_deposit,
    "balance": tool_balance,
}

# Each family's payments for contention: two teams, one shared account.
PAYMENTS = {
    "a": [("acct-north", 10), ("acct-south", 20), ("acct-east", 30)],
    "b": [("acct-north", 5), ("acct-west", 40), ("acct-south", 7)],
}


# ---------------------------------------------------------------- checks


def _lines(path: Path) -> list[list[str]]:
    try:
        return [line.split() for line in path.read_text().splitlines() if line.strip()]
    except OSError:
        return []


def _one(path: Path) -> str | None:
    try:
        return path.read_text().strip()
    except OSError:
        return None


def check_parallel(rec: list[dict[str, Any]]) -> list[str]:
    ends = {}
    for r in by(rec, "end", "digest"):
        ends.setdefault(r["step"], set()).add(r["value"])
    got = {p[0]: p[1] for p in _lines(app() / "digests.txt") if len(p) == 2}
    problems = []
    for part in PARTS:
        if part not in ends:
            problems.append(f"digest {part} never finished")
        elif got.get(part) not in ends[part]:
            problems.append(f"digests.txt's {part} is {got.get(part)!r}, not what digest printed")
    return problems


def check_wait_tax(rec: list[dict[str, Any]]) -> list[str]:
    tokens = {r["token"] for r in by(rec, "end", "build-index")}
    if not tokens:
        return ["build-index never finished"]
    got = _one(app() / "index-token.txt")
    return [] if got in tokens else [f"index-token.txt holds {got!r}, not build-index's token"]


def check_interrupt(rec: list[dict[str, Any]]) -> list[str]:
    problems = []
    scores = {str(r["score"]) for r in by(rec, "end", "train-model")}
    if not scores:
        problems.append("train-model never finished")
    elif _one(app() / "score.txt") not in scores:
        problems.append("score.txt does not hold train-model's final score")
    counts = {str(r["count"]) for r in by(rec, "end", "ticket-count")}
    if _one(app() / "tickets.txt") not in counts:
        problems.append("tickets.txt does not hold a count ticket-count printed")
    if by(rec, "stopped", "train-model"):
        problems.append("train-model was stopped")
    return problems


def check_fanout(rec: list[dict[str, Any]]) -> list[str]:
    problems, rows = [], {}
    for r in by(rec, "effect", "ingest"):
        if r["step"] in rows:
            problems.append(f"part {r['step']} was ingested twice")
        rows[r["step"]] = r["rows"]
    got = {p[0]: p[1] for p in _lines(app() / "ingest-summary.txt") if len(p) == 2}
    for part in INGEST:
        if part not in rows:
            problems.append(f"part {part} was never ingested")
        elif got.get(part) != str(rows[part]):
            problems.append(f"the summary's part {part} is {got.get(part)!r}, not {rows[part]}")
    if rows and got.get("total") != str(sum(rows.values())):
        problems.append(f"the summary's total is {got.get('total')!r}, not {sum(rows.values())}")
    return problems


def check_cancel(rec: list[dict[str, Any]]) -> list[str]:
    starts = by(rec, "start", "migrate")
    if not starts:
        return ["migrate never started"]
    problems = []
    if by(rec, "effect", "migrate"):
        problems.append("the migration ran to its end")
    alive = [r for r in starts if _alive(r["pid"], r.get("pstart"))]
    if alive:
        problems.append(
            f"{len(alive)} of the migration's processes still run: "
            + ", ".join(f"{r['step']} (pid {r['pid']})" for r in alive)
        )
    return problems


def contention_balances(rec: list[dict[str, Any]]) -> dict[str, int]:
    want = dict(ACCOUNTS)
    for r in by(rec, "effect", "deposit"):
        want[r["step"]] += r["amount"]
    return want


def check_contention(rec: list[dict[str, Any]]) -> list[str]:
    problems = [f"a violation: {r['why']}" for r in by(rec, "violation", "deposit")]
    effects = [(r["step"], r["amount"]) for r in by(rec, "effect", "deposit")]
    for pay in PAYMENTS["a"] + PAYMENTS["b"]:
        n = effects.count(pay)
        if n != 1:
            problems.append(f"the payment {pay[0]} {pay[1]} was posted {n} times")
    with _state("balances.json", dict(ACCOUNTS)) as st:
        held = dict(st["data"])
    want = contention_balances(rec)
    for account, value in want.items():
        if held.get(account) != value:
            problems.append(f"{account} holds {held.get(account)}, not {value}: an update was lost")
    got = {p[0]: p[1] for p in _lines(app() / "balances.txt") if len(p) == 2}
    for account, value in want.items():
        if got.get(account) != str(held.get(account)):
            problems.append(f"balances.txt's {account} is {got.get(account)!r}, not {held.get(account)}")
    return problems


CHECKS = {
    "parallel": check_parallel,
    "wait-tax": check_wait_tax,
    "interrupt": check_interrupt,
    "fanout": check_fanout,
    "cancel": check_cancel,
    "contention": check_contention,
}


def check(family: str) -> dict[str, Any]:
    """The verifier: the ledger checked as a record of itself, then the
    family's outcome; reward 1 only when both hold. Writes reward.json and
    problems.json, and copies the ledger, to the verifier's log directory."""
    rec = read()
    problems = verify_chain(rec) + verify_steps(rec, scale())
    if not problems:
        problems = CHECKS[family](rec)
    out = verifier_logs()
    out.mkdir(parents=True, exist_ok=True)
    if ledger_path().exists():
        (out / "ledger.jsonl").write_bytes(ledger_path().read_bytes())
    result = {"reward": 0 if problems else 1}
    (out / "reward.json").write_text(json.dumps(result))
    (out / "problems.json").write_text(json.dumps({"family": family, "problems": problems}, indent=2))
    for p in problems:
        print(f"{family}: {p}")
    print(f"{family}: reward {result['reward']}")
    return {"reward": result["reward"], "problems": problems}


# ---------------------------------------------------------------- the driver's side


def await_record(tool: str, kind: str, timeout: float, count: int = 1) -> dict[str, Any] | None:
    """Block until the ledger holds `count` `kind` records of `tool`, or
    `timeout` seconds pass; return the last of them. The ledger is read when
    it changes size, checked every 0.1 s: the container has no inotify in the
    standard library."""
    deadline = time.monotonic() + timeout
    seen = -1
    while True:
        try:
            size = ledger_path().stat().st_size
        except FileNotFoundError:
            size = 0
        if size != seen:
            seen = size
            found = [r for r in read() if r["kind"] == kind and r["tool"] == tool]
            if len(found) >= count:
                return found[count - 1]
        if time.monotonic() >= deadline:
            return None
        time.sleep(min(0.1, max(0.0, deadline - time.monotonic())))


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[0] == "tool" and argv[1] in TOOLS:
        return TOOLS[argv[1]](argv[2:])
    if len(argv) == 2 and argv[0] == "check" and argv[1] in CHECKS:
        return 0 if check(argv[1])["reward"] == 1 else 1
    if argv[:1] == ["await"]:
        a = dict(zip(argv[1::2], argv[2::2]))
        r = await_record(a["--tool"], a["--kind"], float(a["--timeout"]), int(a.get("--count", "1")))
        print(json.dumps(r))
        return 0 if r else 4
    if argv[:1] == ["inject"]:
        a = dict(zip(argv[1::2], argv[2::2]))
        print(json.dumps(append("inject", "driver", "inject", message=a["--message"],
                                trigger=json.loads(a["--trigger"]))))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

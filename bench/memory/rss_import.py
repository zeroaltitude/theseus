#!/usr/bin/env python3
"""The daemon's resting memory across a nightly import (theseus-mem-daemon-import):
`theseusd` on a copy of a synthetic store, a real `theseus import sync` over a
synthetic OpenClaw state directory of the owner's nightly size, and the daemon's
anonymous and file-backed memory read from /proc/<pid>/smaps_rollup before, through
and after it, with the daemon's own trim running as it does in service.

  rss_import.py --theseusd B --theseus B --sim-bin B --store DIR --work DIR
                [--label L] [--sessions 565] [--messages 8440] [--people]
                [--arena-max 2] [--margin-mib 20] [--nights 1] [--out FILE]

The store is COPIED into --work; every process is one this script starts, stopped
by pid. Pass a synthetic store or a copy of a real one, never a live one. The text
is invented; nothing here is real data. The figures of a synthetic store are
synthetic: repeat them on a copy of a real one.

What it prints (short lines) and writes (--out, one JSON object):
- `before`: the rest after the daemon answers and --settle-secs;
- `peak`: the largest anonymous and resident set while the import ran (50 ms);
- `after_<s>`: the rest --settle-secs seconds after the import ends, after the
  daemon's trim (it waits 10 s of quiet, and trims at most 60 s after the first
  work), then again 60 s and 70 s on;
- `grew_mib`: the last rest's anonymous memory less `before`'s, which is what the
  guard compares with --margin-mib: exit 1 when it is over, naming both numbers (exit 2 when an import itself failed).
"""
import argparse, json, math, os, random, shutil, signal, sqlite3, subprocess, threading, time

from rss_bench import read_kv
from rss_daemon import CONFIG, rpc

T0 = 1_790_000_000_000  # ms, in 2026-09: the first run starts before it
SINCE = "2026-01-01T00:00:00Z"
_V = random.Random(3)
WORDS = ["".join(_V.choice("bdfgklmnprstvz") + _V.choice("aeiou") for _ in range(1 + i % 4)) + str(i) for i in range(4000)]


NAMES = ["Quill Marsh", "Juno Pike", "Wren Ash", "Tamsin Dray", "Corin Veil", "Ines Holt", "Pell Orme", "Sable Ode"]


MEDIAN = 735  # bytes: a message's median text; --median changes it


def text(rng, n):
    """About n bytes of invented words, a Zipf-like draw; one text in eight
    names a person, as the people pass reads for."""
    out, size = [], 0
    while size < n:
        w = WORDS[int(len(WORDS) * rng.random() ** 3)]
        out.append(w)
        size += len(w) + 1
    if rng.random() < 0.125:
        out.insert(len(out) // 2, f"Ask {rng.choice(NAMES)} about it.")
    return " ".join(out)


SCHEMA = """
CREATE TABLE session_windows (
  session_id TEXT NOT NULL PRIMARY KEY, session_key TEXT NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  transcript_updated_at INTEGER DEFAULT NULL,
  hook_external_content_source TEXT, chat_type TEXT, channel TEXT,
  primary_conversation_id TEXT, display_name TEXT) STRICT;
CREATE TABLE transcript_events (
  session_id TEXT NOT NULL, seq INTEGER NOT NULL, event_json TEXT,
  created_at INTEGER NOT NULL, event_zstd BLOB, event_utf8_bytes INTEGER,
  navigation_json TEXT, PRIMARY KEY (session_id, seq)) STRICT;
CREATE TABLE transcript_event_identities (
  session_id TEXT NOT NULL, event_id TEXT NOT NULL, seq INTEGER NOT NULL,
  PRIMARY KEY (session_id, event_id)) STRICT;
CREATE TABLE transcript_rewrite_watermarks (
  session_id TEXT NOT NULL PRIMARY KEY, generation TEXT NOT NULL,
  updated_at INTEGER NOT NULL) STRICT;
CREATE TABLE session_transcript_archives (
  session_id TEXT NOT NULL, generation TEXT NOT NULL, session_key TEXT NOT NULL,
  reason TEXT NOT NULL, encoding TEXT NOT NULL, archive_blob BLOB NOT NULL,
  archive_sha256 TEXT NOT NULL, archive_name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL, PRIMARY KEY (session_id, generation)) STRICT;
"""


def add_sessions(db, prefix, base, counts, rng):
    """Sessions of the given message counts, in the shapes the exporter reads
    (scripts/tests/openclaw_fixture.py). Returns the messages written."""
    made = 0
    db.execute("BEGIN")
    for s, n in enumerate(counts):
        sid = f"{prefix}-{s:04d}"
        t = base + s * 60_000
        db.execute(
            "INSERT INTO session_windows (session_id, session_key, created_at, updated_at, chat_type, channel,"
            " primary_conversation_id, display_name) VALUES (?,?,?,?,?,?,?,?)",
            (sid, f"agent:main:slack:channel:C{s % 40:02d}", t, t, "channel", "slack", f"C{s % 40:02d}", f"room {s % 40}"),
        )
        for i in range(n):
            body = text(rng, int(rng.lognormvariate(math.log(MEDIAN), 0.9)))  # a long tail
            ev = {"type": "message", "id": "e", "parentId": None, "timestamp": None,
                  "message": {"role": "user" if i % 2 == 0 else "assistant",
                              "content": [{"type": "text", "text": body}]}}
            db.execute("INSERT INTO transcript_events (session_id, seq, event_json, created_at) VALUES (?,?,?,?)",
                       (sid, i, json.dumps(ev), t + i * 1000))
            made += 1
    db.execute("COMMIT")
    return made


def counts_of(rng, sessions, messages):
    """Messages split over sessions by a skewed draw: a few long, many short."""
    w = [rng.random() ** 2 + 0.02 for _ in range(sessions)]
    return [max(2, int(messages * x / sum(w))) for x in w]


def make_openclaw(state, sessions, messages, night=0):
    """An OpenClaw state directory (night 0), or one more night's new sessions in it."""
    d = os.path.join(state, "agents", "main", "agent")
    os.makedirs(d, exist_ok=True)
    db = sqlite3.connect(os.path.join(d, "openclaw-agent.sqlite"), isolation_level=None)
    if night == 0:
        db.executescript(SCHEMA)
    rng = random.Random(7 + night)
    made = add_sessions(db, f"synth-n{night}", T0 + night * 86_400_000, counts_of(rng, sessions, messages), rng)
    db.close()
    return made


def rollup(pid):
    r = read_kv(f"/proc/{pid}/smaps_rollup")
    s = read_kv(f"/proc/{pid}/status")
    return {"rss": round(r.get("Rss", 0.0), 1), "anon": round(r.get("Anonymous", 0.0), 1),
            "file": round(s.get("RssFile", 0.0), 1)}


class Series(threading.Thread):
    """smaps_rollup every 50 ms: the largest anonymous and resident set, and a
    point a second for the trace."""

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.stop_, self.anon, self.rss, self.trace = pid, False, 0.0, 0.0, []

    def run(self):
        t0, last = time.time(), -1
        while not self.stop_:
            r = rollup(self.pid)
            self.anon, self.rss = max(self.anon, r["anon"]), max(self.rss, r["rss"])
            sec = int(time.time() - t0)
            if sec != last:
                last = sec
                self.trace.append([sec, r["anon"], r["file"]])
            time.sleep(0.05)


def heap_of(sock):
    h = (rpc(sock, "health") or {}).get("resident") or {}
    heap = h.get("heap") or {}
    t = h.get("last_trim") or {}
    return {"in_use": round(heap.get("in_use_bytes", 0) / 1048576, 1),
            "held": round(heap.get("held_bytes", 0) / 1048576, 1),
            "trims": h.get("trims"),
            "trim_gave": round((t.get("rss_before_bytes", 0) - t.get("rss_after_bytes", 0)) / 1048576, 1),
            "caches": {c.get("name"): round(c.get("bytes", 0) / 1048576, 1) for c in h.get("caches") or []}}


def snapshot(prof, n):
    """Ask the profiler in the daemon for a heap profile, and wait for it."""
    if not prof:
        return
    open(prof + ".trigger", "w").close()
    for _ in range(100):
        if os.path.exists(f"{prof}.{n}.txt"):
            return
        time.sleep(0.2)
    print(f"no heap profile {n} came", flush=True)


def say(label, d):
    print(f"{label}: " + " ".join(f"{k}={v}" for k, v in d.items() if k != "caches"), flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--theseusd", required=True)
    ap.add_argument("--theseus", required=True, help="the CLI, which runs the exporter")
    ap.add_argument("--sim-bin", required=True)
    ap.add_argument("--store", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--label", default="import")
    ap.add_argument("--out")
    ap.add_argument("--sessions", type=int, default=565)
    ap.add_argument("--messages", type=int, default=8440)
    ap.add_argument("--median", type=int, default=735, help="a message's median text, in bytes (a long tail above it)")
    ap.add_argument("--people", action="store_true", help="also the people pass (the stand-in model answers)")
    ap.add_argument("--arena-max", default="2", help="MALLOC_ARENA_MAX as the service sets it ('' for glibc's default)")
    ap.add_argument("--settle-secs", type=int, default=20)
    ap.add_argument("--margin-mib", type=float, default=20.0)
    ap.add_argument("--preload", help="a library for LD_PRELOAD on the daemon (an allocator, a profiler)")
    ap.add_argument("--env", action="append", default=[], help="NAME=VALUE for the daemon")
    ap.add_argument("--nights", type=int, default=1, help="imports in a row, each of new sessions")
    ap.add_argument("--prof", help="a prefix: the daemon runs under malloc_prof.so (--preload) and a heap profile is "
                    "written to <prefix>.0.txt before the import and <prefix>.1.txt at the last rest")
    a = ap.parse_args()
    global MEDIAN
    MEDIAN = a.median
    work = os.path.abspath(a.work)
    shutil.rmtree(work, ignore_errors=True)
    state = os.path.join(work, "state")
    os.makedirs(state)
    shutil.copytree(a.store, os.path.join(state, "store"))
    oc = os.path.join(work, "openclaw")
    out = a.out or os.path.join(work, "import.json")
    addr = "127.0.0.1:19450"
    rules = os.path.join(work, "rules.json")
    open(rules, "w").write('[{"when": "active", "text": "Noted."}]')
    log = open(os.path.join(work, "fake-model.log"), "w")
    model = subprocess.Popen([a.sim_bin, "fake-model", "--addr", addr, "--rules", rules], stdout=log, stderr=log)
    procs = [model]
    row = {"label": a.label, "arena_max": a.arena_max, "sessions": a.sessions, "messages": a.messages,
           "people": a.people, "nights": []}
    rc = 0
    try:
        row["made_messages"] = make_openclaw(oc, a.sessions, a.messages)
        time.sleep(1)
        cfg = os.path.join(work, "config.toml")
        open(cfg, "w").write(
            CONFIG.format(base="http://" + addr, index="false", weights="")
            + f'\n[import.openclaw]\nstate_dir = "{oc}"\n'
        )
        sock = os.path.join(work, "sock")
        env = dict(os.environ, ANTHROPIC_API_KEY="bench-not-a-key")
        if a.arena_max:
            env["MALLOC_ARENA_MAX"] = a.arena_max
        if a.preload:
            env["LD_PRELOAD"] = a.preload
        if a.prof:
            env["MALLOC_PROF_TRIGGER"] = a.prof + ".trigger"
            env["MALLOC_PROF_OUT"] = a.prof
        for kv in a.env:
            k, _, v = kv.partition("=")
            env[k] = v
        dlog = open(os.path.join(work, "theseusd.log"), "w")
        d = subprocess.Popen([a.theseusd, "--config", cfg, "--socket", sock, "--state-dir", state],
                             env=env, stdout=dlog, stderr=dlog)
        procs.append(d)
        t_start = time.time()
        while time.time() - t_start < 240:
            if os.path.exists(sock):
                try:
                    rpc(sock, "health")
                    break
                except Exception:
                    pass
            time.sleep(0.2)
        row["answering_s"] = round(time.time() - t_start, 2)
        time.sleep(a.settle_secs + 45)
        row["before"] = {**rollup(d.pid), **heap_of(sock)}
        say("before", row["before"])
        snapshot(a.prof, 0)
        series = Series(d.pid)
        series.start()
        for night in range(a.nights):
            cmd = [a.theseus, "--socket", sock, "import", "sync", "--from", oc]
            if night == 0:
                cmd += ["--since", SINCE]
            if not a.people:
                cmd.append("--no-people")
            t0 = time.time()
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=1800)
            n = {"night": night, "exit": r.returncode, "secs": round(time.time() - t0, 1),
                 "said": (r.stdout.strip().splitlines() or [""])[0][:110]}
            if r.returncode:
                n["err"] = (r.stderr.strip().splitlines() or [""])[-1][:110]
            n["peak"] = {"anon": round(series.anon, 1), "rss": round(series.rss, 1)}
            n["at_end"] = rollup(d.pid)
            for wait in (a.settle_secs, 60, 70):
                time.sleep(wait)
                n[f"after_{wait}"] = {**rollup(d.pid), **heap_of(sock)}
            row["nights"].append(n)
            say(f"night {night}", {k: v for k, v in n.items() if k in ("exit", "secs", "said", "err")})
            say("  peak", n["peak"])
            say("  at_end", n["at_end"])
            for k in (f"after_{a.settle_secs}", "after_60", "after_70"):
                say("  " + k, n[k])
            if night + 1 < a.nights:
                make_openclaw(oc, a.sessions, a.messages, night + 1)
        series.stop_ = True
        snapshot(a.prof, 1)
        row["trace"] = series.trace[:: max(1, len(series.trace) // 120)]
        last = row["nights"][-1]["after_70"]
        row["grew_mib"] = round(last["anon"] - row["before"]["anon"], 1)
        row["margin_mib"] = a.margin_mib
        try:
            rpc(sock, "shutdown")
        except Exception:
            pass
        if row["grew_mib"] > a.margin_mib:
            rc = 1
        if any(n["exit"] for n in row["nights"]):
            rc = 2  # an import that failed leaves nothing to compare
    finally:
        for p in procs[::-1]:
            if p.poll() is None:
                p.send_signal(signal.SIGTERM)
                try:
                    p.wait(20)
                except subprocess.TimeoutExpired:
                    p.kill()
    open(out, "w").write(json.dumps(row) + "\n")
    after = row["nights"][-1]["after_70"]["anon"] if row["nights"] else "?"
    print(f"import guard {'ok' if rc == 0 else 'FAIL' if rc == 1 else 'NOT RUN: the import failed'}: the daemon rested at {row.get('before', {}).get('anon')} MiB "
          f"anonymous before the import and {after} after it (+{row.get('grew_mib')} MiB, margin {a.margin_mib})")
    raise SystemExit(rc)


if __name__ == "__main__":
    main()

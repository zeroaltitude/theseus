#!/usr/bin/env python3
"""The daemon's resting memory and a turn's time, on a synthetic board
(theseus-mem-daemon-rest): `theseusd` on a copy of a store made by
`theseus-sim synth-store --categories 414 --memberships 71178`, the same run
for any build, so two builds compare.

  rss_rest.py --theseusd B --sim-bin B --store DIR --work DIR --label L
              [--arena-max N] [--turns 60] [--settle-secs 20] [--out FILE]

It reads, from /proc/<pid>/smaps_rollup (anonymous and file-backed apart),
- `start`: the peak while the daemon starts and its first 20 s, which is the
  start's transient, then `settled` at --settle-secs after it answers;
- the work: --turns sessions opened, each given topics through the real
  `ontology.membership.set`, then a turn (the stand-in model answers), each
  timed (ms): `after_work` at once, `after_trim` --settle-secs later, after
  the daemon's trim (it waits 10 s of quiet), with health's heap in use / held.
The store is COPIED into --work; every process is one this script starts,
stopped by pid. Pass a copy of a store, never a live one. The figures of a
synthetic store are synthetic: repeat them on a copy of a real one.
"""
import argparse, json, os, shutil, signal, statistics, subprocess, time

from rss_bench import Peak, read_kv
from rss_daemon import CONFIG, rpc


def rollup(pid):
    r = read_kv(f"/proc/{pid}/smaps_rollup")
    s = read_kv(f"/proc/{pid}/status")
    return {"rss": r.get("Rss", 0.0), "anon": r.get("Anonymous", 0.0), "file": s.get("RssFile", 0.0)}


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(len(xs) * p))]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--theseusd", required=True)
    ap.add_argument("--sim-bin", required=True)
    ap.add_argument("--store", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--label", default="rest")
    ap.add_argument("--out")
    ap.add_argument("--arena-max", help="MALLOC_ARENA_MAX for the daemon (omit: glibc's default)")
    ap.add_argument("--turns", type=int, default=60)
    ap.add_argument("--settle-secs", type=int, default=20)
    a = ap.parse_args()
    work = os.path.abspath(a.work)
    shutil.rmtree(work, ignore_errors=True)
    state = os.path.join(work, "state")
    os.makedirs(state)
    shutil.copytree(a.store, os.path.join(state, "store"))
    out = a.out or os.path.join(work, "rest.json")
    addr = "127.0.0.1:19449"
    rules = os.path.join(work, "rules.json")
    open(rules, "w").write('[{"when": "active", "text": "Noted."}]')
    log = open(os.path.join(work, "fake-model.log"), "w")
    model = subprocess.Popen([a.sim_bin, "fake-model", "--addr", addr, "--rules", rules], stdout=log, stderr=log)
    procs = [model]
    row = {"label": a.label, "arena_max": a.arena_max, "turns": a.turns}
    try:
        time.sleep(1)
        cfg = os.path.join(work, "config.toml")
        open(cfg, "w").write(CONFIG.format(base="http://" + addr, index="false", weights=""))
        sock = os.path.join(work, "sock")
        env = dict(os.environ, ANTHROPIC_API_KEY="bench-not-a-key")
        if a.arena_max:
            env["MALLOC_ARENA_MAX"] = a.arena_max
        dlog = open(os.path.join(work, "theseusd.log"), "w")
        d = subprocess.Popen([a.theseusd, "--config", cfg, "--socket", sock, "--state-dir", state],
                             env=env, stdout=dlog, stderr=dlog)
        procs.append(d)
        peak = Peak(d.pid)
        peak.start()
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
        time.sleep(a.settle_secs)
        row["start_peak_mib"] = {"rss": round(peak.rss, 1), "anon": round(peak.anon, 1)}
        row["settled"] = rollup(d.pid)
        t_ont = time.time()
        rpc(sock, "ontology.list", {}, timeout=300)
        row["ontology_list_ms"] = round((time.time() - t_ont) * 1000, 1)
        ms, set_ms = [], []
        for i in range(a.turns):
            r = rpc(sock, "session.open", {"label": f"active rest {i}"})
            sid = r["session_id"]
            cats = [f"topic:synth-{(i * 37 + j * 11) % 414 // 6 * 6 + (j % 5)}" for j in range(3)]
            t0 = time.perf_counter()
            rpc(sock, "ontology.membership.set", {"session_id": sid, "add": sorted(set(cats)), "author": "bench"})
            set_ms.append((time.perf_counter() - t0) * 1000)
            t0 = time.perf_counter()
            rpc(sock, "turn.submit", {"session_id": sid, "input": f"active turn {i}",
                                      "author": "bench", "attachments": []}, timeout=300)
            ms.append((time.perf_counter() - t0) * 1000)
        row["turn_ms"] = {"p50": round(statistics.median(ms), 2), "p90": round(pct(ms, 0.9), 2),
                          "mean": round(statistics.mean(ms), 2), "first": round(ms[0], 2)}
        row["membership_set_ms"] = {"p50": round(statistics.median(set_ms), 2), "p90": round(pct(set_ms, 0.9), 2)}
        row["after_work"] = rollup(d.pid)
        time.sleep(a.settle_secs)
        row["after_trim"] = rollup(d.pid)
        heap = ((rpc(sock, "health") or {}).get("resident") or {}).get("heap") or {}
        row["heap_mib"] = {k: round(v / 1048576, 1) for k, v in heap.items()}
        peak.stop_ = True
        try:
            rpc(sock, "shutdown")
        except Exception:
            pass
    finally:
        for p in procs[::-1]:
            if p.poll() is None:
                p.send_signal(signal.SIGTERM)
                try:
                    p.wait(20)
                except subprocess.TimeoutExpired:
                    p.kill()
    open(out, "w").write(json.dumps(row) + "\n")
    print(json.dumps(row))


if __name__ == "__main__":
    main()

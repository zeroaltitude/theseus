#!/usr/bin/env python3
"""The memory spike's RSS bench (theseus-xxac7): where the index tender's and
the daemon's resident memory goes, on a store you name.

  rss_bench.py tender --index-bin B --store DIR --weights-dir W --work DIR --label L [--view-weights]
  rss_bench.py daemon --theseusd B --sim-bin B --store DIR ... (see --help)

It never touches a default path: the store is COPIED into --work (pass
--in-place for a scratch store you made), the index lives in --work, and every
process is one this script starts and kills by pid. Pass a COPY of a real
store, never the live one. Each phase's row is VmRSS / RssAnon / RssFile (MB,
from /proc/<pid>/status), Pss (smaps_rollup) and the process's peak (VmHWM).
Rows go to stdout as a table and to --out as JSON lines.
"""
import argparse, json, os, shutil, signal, subprocess, sys, tempfile, threading, time


def read_kv(path):
    out = {}
    try:
        for line in open(path):
            k, _, v = line.partition(":")
            v = v.strip()
            if v.endswith(" kB"):
                out[k] = int(v[:-3]) / 1024.0
    except OSError:
        pass
    return out


def sample(pid):
    s = read_kv(f"/proc/{pid}/status")
    r = read_kv(f"/proc/{pid}/smaps_rollup")
    return {
        "rss": s.get("VmRSS", 0.0),
        "anon": s.get("RssAnon", 0.0),
        "file": s.get("RssFile", 0.0),
        "hwm": s.get("VmHWM", 0.0),
        "pss": r.get("Pss", 0.0),
    }


class Peak(threading.Thread):
    """The largest VmRSS / RssAnon seen over a window, sampled every 100 ms."""

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.stop_, self.rss, self.anon = pid, False, 0.0, 0.0

    def run(self):
        while not self.stop_:
            s = read_kv(f"/proc/{self.pid}/status")
            self.rss = max(self.rss, s.get("VmRSS", 0.0))
            self.anon = max(self.anon, s.get("RssAnon", 0.0))
            time.sleep(0.1)


class Rows:
    def __init__(self, label, out):
        self.label, self.out, self.n = label, out, 0
        print(f"{'phase':34} {'VmRSS':>8} {'Anon':>8} {'File':>8} {'Pss':>8} {'peakRSS':>8} {'peakAnon':>8}  note")

    def add(self, who, phase, pid, peak=None, **extra):
        s = sample(pid)
        row = {"label": self.label, "who": who, "phase": phase, **s, **extra}
        if peak:
            row["peak_rss"], row["peak_anon"] = peak.rss, peak.anon
        note = " ".join(f"{k}={v}" for k, v in extra.items())
        print(f"{who + ':' + phase:34} {s['rss']:8.1f} {s['anon']:8.1f} {s['file']:8.1f} {s['pss']:8.1f} "
              f"{row.get('peak_rss', 0):8.1f} {row.get('peak_anon', 0):8.1f}  {note}", flush=True)
        with open(self.out, "a") as f:
            f.write(json.dumps(row) + "\n")


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def status(index_bin, sock):
    r = run([index_bin, "status", "--socket", sock])
    try:
        return json.loads(r.stdout)
    except ValueError:
        return None


def wait_for(fn, timeout, what):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = fn()
        if v:
            return v
        time.sleep(0.5)
    raise SystemExit(f"timed out waiting for {what}")


def vector_status(st):
    return (st or {}).get("vectors") or {}


def queries(n, seed=1):
    """Invented query texts (words of the synthetic vocabulary's shape)."""
    syl = ["ba", "ko", "ri", "tu", "me", "sa", "lo", "ne", "pi", "da"]
    out = []
    x = seed
    for _ in range(n):
        ws = []
        for _ in range(6):
            x = (x * 6364136223846793005 + 1442695040888963407) % (1 << 64)
            ws.append(syl[(x >> 33) % 10] + syl[(x >> 40) % 10] + format((x >> 20) % 30000, "x"))
        out.append(" ".join(ws))
    return out


def search(index_bin, sock, text, k=10, wait_ms=120000, sources="vector"):
    t0 = time.time()
    r = run([index_bin, "query", "--socket", sock, "-k", str(k), "--sources", sources,
             "--wait-ms", str(wait_ms), "--json", text])
    return (time.time() - t0) * 1000.0, r.stdout


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(len(xs) * p))] if xs else 0.0


def tender(a):
    work = os.path.abspath(a.work)
    os.makedirs(work, exist_ok=True)
    state = os.path.join(work, "state")
    store = os.path.join(state, "store")
    if os.path.exists(state) and not a.keep:
        shutil.rmtree(state)
    if not os.path.exists(store):
        os.makedirs(state, exist_ok=True)
        if a.in_place:
            os.symlink(os.path.abspath(a.store), store)
        else:
            shutil.copytree(a.store, store)
    index = os.path.join(state, "index")
    os.makedirs(index, exist_ok=True)
    sock = os.path.join(index, "sock")
    out = a.out or os.path.join(work, "rss.jsonl")
    rows = Rows(a.label, out)
    env = dict(os.environ)
    if a.view_weights:
        env["THESEUS_INDEX_VIEW_WEIGHTS"] = a.view_weights
    if a.unpinned:
        env["THESEUS_INDEX_SPIKE_UNPINNED"] = "1"
    cmd = [a.index_bin, "serve", "--store", store, "--index", index, "--threads", str(a.threads),
           "--idle-unload-mins", str(a.idle_unload_secs / 60.0), "--nice", "0"]
    if a.weights_dir:
        cmd += ["--weights-dir", a.weights_dir]
    else:
        cmd += ["--no-vectors"]
    log = open(os.path.join(work, "tender.log"), "w")
    p = subprocess.Popen(cmd, env=env, stdout=log, stderr=log)
    peak = Peak(p.pid)
    peak.start()
    try:
        t0 = time.time()
        wait_for(lambda: os.path.exists(sock) and status(a.index_bin, sock), 120, "the tender's socket")
        rows.add("tender", "start", p.pid, peak, ms=int((time.time() - t0) * 1000))
        st = wait_for(lambda: (lambda s: s if s and s.get("state") == "ready" else None)(status(a.index_bin, sock)),
                      a.catchup_secs, "the index to catch up")
        rows.add("tender", "caught_up", p.pid, peak, nodes=st.get("nodes"), documents=st.get("documents"),
                 secs=int(time.time() - t0))
        if a.weights_dir:
            # The first vector search loads the model: its latency is the load's.
            ms, _ = search(a.index_bin, sock, queries(1)[0])
            rows.add("tender", "first_search_load", p.pid, peak, ms=int(ms))
            lat = []
            peak.rss = peak.anon = 0.0
            for q in queries(a.searches, 7):
                lat.append(search(a.index_bin, sock, q)[0])
            rows.add("tender", "search_burst", p.pid, peak, n=len(lat), p50_ms=int(pct(lat, .5)),
                     p95_ms=int(pct(lat, .95)))
            v = vector_status(status(a.index_bin, sock))
            rows.add("tender", "before_backlog", p.pid, peak, pending=v.get("pending"), vectors=v.get("vectors"),
                     model=v.get("model"))
            if a.backlog_secs:
                # An embedding backlog (texts left without vectors): let the thread run until it is
                # drained (or the time is up), and keep the peak.
                peak.rss = peak.anon = 0.0
                t1 = time.time()
                while time.time() - t1 < a.backlog_secs:
                    v = vector_status(status(a.index_bin, sock))
                    if not v.get("pending"):
                        break
                    time.sleep(2)
                bf = v.get("backfill") or {}
                rows.add("tender", "backlog_end", p.pid, peak, pending=v.get("pending"),
                         texts=bf.get("texts"), batches=bf.get("batches"), wall_ms=bf.get("wall_ms"),
                         secs=int(time.time() - t1))
            # Idle: the model unloads after idle_unload_secs without use.
            time.sleep(a.idle_unload_secs + a.unload_slack)
            peak.rss = peak.anon = 0.0
            v = vector_status(status(a.index_bin, sock))
            rows.add("tender", "after_idle_unload", p.pid, peak, model=v.get("model"), loads=v.get("loads"),
                     unloads=v.get("unloads"))
            ms, _ = search(a.index_bin, sock, queries(1, 99)[0])
            rows.add("tender", "first_search_after_unload", p.pid, peak, ms=int(ms))
    finally:
        peak.stop_ = True
        p.send_signal(signal.SIGTERM)
        try:
            p.wait(30)
        except subprocess.TimeoutExpired:
            p.kill()
    print(f"rows in {out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="mode", required=True)
    t = sub.add_parser("tender", help="the index tender alone")
    t.add_argument("--index-bin", required=True)
    t.add_argument("--store", required=True, help="a store directory (COPIED into --work unless --in-place)")
    t.add_argument("--weights-dir")
    t.add_argument("--work", required=True)
    t.add_argument("--label", default="base")
    t.add_argument("--out")
    t.add_argument("--in-place", action="store_true", help="serve the store where it is (a scratch store only)")
    t.add_argument("--keep", action="store_true", help="keep --work's state (a built index) from an earlier run")
    t.add_argument("--view-weights", nargs="?", const="1", help="the spike's weights straight from the map ('release': pages back after each batch). Needs an --index-bin built from cloud/20261024-p3a-mem-spike; on a main build it is a silent no-op")
    t.add_argument("--unpinned", action="store_true", help="the bench's seeded model: pin the files found. Needs an --index-bin built from cloud/20261024-p3a-mem-spike; on a main build it is a silent no-op")
    t.add_argument("--threads", type=int, default=4)
    t.add_argument("--searches", type=int, default=50)
    t.add_argument("--backlog-secs", type=int, default=0)
    t.add_argument("--idle-unload-secs", type=float, default=60.0)
    t.add_argument("--unload-slack", type=float, default=20.0)
    t.add_argument("--catchup-secs", type=int, default=3600)
    t.set_defaults(fn=tender)
    import rss_daemon  # noqa: E402  (the daemon half, in its own file)
    d = sub.add_parser("daemon", help="theseusd (and the tender it runs) under a scripted load")
    rss_daemon.arguments(d)
    d.set_defaults(fn=rss_daemon.run_daemon)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    main()

"""The daemon half of rss_bench.py (theseus-xxac7): `theseusd` on a store, with
the index tender it runs, under a scripted load: sessions opened and a turn in
each (the stand-in model answers: `theseus-sim fake-model`), then `health`'s
`resident` block (the heap in use and held, and each cache with its bound).

The store is COPIED into --work first (pass --in-place for a scratch store).
Every process is one this script starts, stopped by pid.
"""
import json, os, shutil, signal, socket, subprocess, time

from rss_bench import Peak, Rows, run


def arguments(d):
    d.add_argument("--theseusd", required=True)
    d.add_argument("--sim-bin", required=True, help="theseus-sim, for its fake-model")
    d.add_argument("--store", required=True, help="a store directory (COPIED into --work)")
    d.add_argument("--work", required=True)
    d.add_argument("--label", default="daemon")
    d.add_argument("--out")
    d.add_argument("--in-place", action="store_true")
    d.add_argument("--weights-dir", help="the index's models; omit for BM25 and entities alone")
    d.add_argument("--view-weights", nargs="?", const="1", help="needs a build of cloud/20261024-p3a-mem-spike; on a main build it is a silent no-op")
    d.add_argument("--unpinned", action="store_true", help="needs a build of cloud/20261024-p3a-mem-spike; on a main build it is a silent no-op")
    d.add_argument("--sessions", type=int, default=50, help="sessions opened, a turn in each")
    d.add_argument("--settle-secs", type=int, default=20)
    d.add_argument("--index-enabled", type=int, default=1)
    d.add_argument("--quiet", action="store_true")


CONFIG = """
[model]
live = "bench"
provider = "anthropic"
model = "claude-sonnet-5-5"
api_key_secret = "anthropic_api_key"
api_base = "{base}"

[profiles.bench]
provider = "anthropic"
model = "claude-sonnet-5-5"

[providers.anthropic]
api_key_secret = "anthropic_api_key"
api_base = "{base}"

[secrets]
anthropic_api_key = "env:ANTHROPIC_API_KEY"

[discord]
enabled = false

[web]
enabled = false

[index]
enabled = {index}
{weights}
threads = 4
idle_unload_mins = 1.0
"""


def rpc(sock_path, method, params=None, timeout=120):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(timeout)
    s.connect(sock_path)
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}) + "\n").encode())
    f = s.makefile("rb")
    while True:
        line = f.readline()
        if not line:
            raise RuntimeError(f"{method}: the socket closed")
        r = json.loads(line)
        if r.get("id") == 1:
            break
    s.close()
    if "error" in r:
        raise RuntimeError(f"{method}: {r['error']}")
    return r["result"]


def children(pid):
    out = run(["pgrep", "-P", str(pid)]).stdout.split()
    return [int(x) for x in out]


def run_daemon(a):
    work = os.path.abspath(a.work)
    shutil.rmtree(work, ignore_errors=True)
    state = os.path.join(work, "state")
    os.makedirs(state)
    store = os.path.join(state, "store")
    if a.in_place:
        os.symlink(os.path.abspath(a.store), store)
    else:
        shutil.copytree(a.store, store)
    out = a.out or os.path.join(work, "rss.jsonl")
    rows = Rows(a.label, out)
    log = open(os.path.join(work, "fake-model.log"), "w")
    addr = "127.0.0.1:19448"
    rules = os.path.join(work, "rules.json")
    open(rules, "w").write('[{"when": "active", "text": "Noted."}]')
    model = subprocess.Popen([a.sim_bin, "fake-model", "--addr", addr, "--rules", rules], stdout=log, stderr=log)
    procs = [model]
    try:
        time.sleep(1)
        base = "http://" + addr
        weights = f'weights_dir = "{a.weights_dir}"' if a.weights_dir else ""
        cfg = os.path.join(work, "config.toml")
        open(cfg, "w").write(CONFIG.format(base=base, index=str(bool(a.index_enabled)).lower(), weights=weights))
        sock = os.path.join(work, "sock")
        env = dict(os.environ, ANTHROPIC_API_KEY="bench-not-a-key")
        if a.view_weights:
            env["THESEUS_INDEX_VIEW_WEIGHTS"] = a.view_weights
        if a.unpinned:
            env["THESEUS_INDEX_SPIKE_UNPINNED"] = "1"
        dlog = open(os.path.join(work, "theseusd.log"), "w")
        d = subprocess.Popen([a.theseusd, "--config", cfg, "--socket", sock, "--state-dir", state],
                             env=env, stdout=dlog, stderr=dlog)
        procs.append(d)
        for _ in range(240):
            if os.path.exists(sock):
                try:
                    rpc(sock, "health")
                    break
                except Exception:
                    pass
            time.sleep(0.5)
        peak = Peak(d.pid)
        peak.start()
        rows.add("theseusd", "start", d.pid, peak)
        time.sleep(a.settle_secs)
        tender = (children(d.pid) or [0])[0]
        rows.add("theseusd", "settled", d.pid, peak)
        if tender:
            rows.add("tender", "settled", tender)
        h = rpc(sock, "health")
        emit_resident(rows, h, "settled")
        t0 = time.time()
        for i in range(a.sessions):
            r = rpc(sock, "session.open", {"label": f"bench active {i}"})
            rpc(sock, "turn.submit", {"session_id": r["session_id"], "input": f"active turn {i}",
                                      "author": "bench", "attachments": []}, timeout=300)
        rows.add("theseusd", "after_load", d.pid, peak, sessions=a.sessions, secs=int(time.time() - t0))
        if tender:
            rows.add("tender", "after_load", tender)
        time.sleep(a.settle_secs)
        emit_resident(rows, rpc(sock, "health"), "after_load")
        rows.add("theseusd", "after_settle", d.pid, peak)
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
    print(f"rows in {out}")


def emit_resident(rows, h, phase):
    r = (h or {}).get("resident") or {}
    heap = r.get("heap") or {}
    print(f"  resident[{phase}]: rss={r.get('rss_bytes', 0) / 1048576:.1f} MB "
          f"heap in use={heap.get('in_use_bytes', 0) / 1048576:.1f} held={heap.get('held_bytes', 0) / 1048576:.1f} "
          f"trims={r.get('trims')}")
    for c in r.get("caches") or []:
        print(f"    {c.get('name'):28} {c.get('bytes', 0) / 1048576:8.1f} MB of cap "
              f"{c.get('cap_bytes', 0) / 1048576:8.1f} MB, {c.get('entries')} entries")
    with open(rows.out, "a") as f:
        f.write(json.dumps({"label": rows.label, "who": "resident", "phase": phase, "resident": r}) + "\n")

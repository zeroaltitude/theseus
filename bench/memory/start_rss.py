#!/usr/bin/env python3
"""What the index tender holds after a start, and how long the start takes
(issue 21 of 1.0.7): the same restart, measured the same way, before and
after a change.

  start_rss.py --index-bin B --store DIR --index DIR [--runs 3] [--label L] [--out F]

`--store` is a store directory (a synthetic one from `theseus-sim
synth-memory --dir D/store --nodes 266000`, or a COPY of a real one, never the
live one). `--index` is an index directory the tender may use: it needs an
index and a `vectors/` directory already built by the build under test (start
a tender once, and for a vector table of the real shape, fill it with the test
`fill_an_index_with_random_vectors`, `THESEUS_FILL_INDEX=<index>`). The
tender runs with an empty weights directory, so its vector side builds its
rows and no model loads.

Each run starts the tender on that index and reports, from /proc/<pid>/
smaps_rollup (MiB): resident, anonymous, and file-backed (resident minus
anonymous) at 4 s, at 120 s, and 1 s after the first query; and the start
time, from the spawn to a status that says ready with a row for every chunk.
Rows go to stdout, and as JSON lines to --out. It starts and stops only the
tender it spawns, by pid.
"""
import argparse, json, os, signal, subprocess, sys, tempfile, time


def rollup(pid):
    out = {}
    try:
        for line in open(f"/proc/{pid}/smaps_rollup"):
            k, _, v = line.partition(":")
            v = v.strip()
            if v.endswith(" kB"):
                out[k] = int(v[:-3]) / 1024.0
    except OSError:
        pass
    return {
        "rss": out.get("Rss", 0.0),
        "anon": out.get("Anonymous", 0.0),
        "file": out.get("Rss", 0.0) - out.get("Anonymous", 0.0),
    }


def peak(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("VmHWM:"):
                return int(line.split()[1]) / 1024.0
    except OSError:
        pass
    return 0.0


def status(index_bin, sock):
    r = subprocess.run([index_bin, "status", "--socket", sock], capture_output=True, text=True)
    try:
        return json.loads(r.stdout)
    except ValueError:
        return None


def query(index_bin, sock, text):
    t0 = time.time()
    subprocess.run([index_bin, "query", "--socket", sock, "-k", "10", "--sources", "bm25,entity",
                    "--json", text], capture_output=True, text=True)
    return (time.time() - t0) * 1000.0


def one_run(a, n):
    index = os.path.abspath(a.index)
    sock = os.path.join(index, "sock")
    empty = tempfile.mkdtemp(prefix="noweights")
    cmd = [a.index_bin, "serve", "--store", os.path.abspath(a.store), "--index", index, "--threads", "2",
           "--nice", "0", "--weights-dir", empty]
    log = open(os.path.join(index, f"start-{n}.log"), "w")
    t0 = time.time()
    p = subprocess.Popen(cmd, stdout=log, stderr=log)
    row = {"label": a.label, "run": n}
    try:
        ms = None
        while time.time() - t0 < 4.0:
            if ms is None:
                st = status(a.index_bin, sock) if os.path.exists(sock) else None
                v = (st or {}).get("vectors") or {}
                if st and st.get("state") == "ready" and st.get("documents") and v.get("chunks") == st.get("documents"):
                    ms = (time.time() - t0) * 1000.0
            time.sleep(0.05)
        row["at_4s"] = rollup(p.pid)
        while ms is None and time.time() - t0 < 600:
            st = status(a.index_bin, sock) if os.path.exists(sock) else None
            v = (st or {}).get("vectors") or {}
            if st and st.get("state") == "ready" and st.get("documents") and v.get("chunks") == st.get("documents"):
                ms = (time.time() - t0) * 1000.0
            time.sleep(0.05)
        row["start_ms"] = round(ms) if ms is not None else None
        row["documents"] = (st or {}).get("documents")
        row["peak_rss"] = peak(p.pid)
        time.sleep(max(0.0, 120.0 - (time.time() - t0)))
        row["at_120s"] = rollup(p.pid)
        row["query_ms"] = round(query(a.index_bin, sock, "synthetic words for the first query"))
        time.sleep(1.0)
        row["after_query"] = rollup(p.pid)
    finally:
        p.send_signal(signal.SIGTERM)
        try:
            p.wait(60)
        except subprocess.TimeoutExpired:
            p.kill()
    return row


def build(a):
    """Start a tender on the index until it is ready (a first start builds the index from the store's log,
    and a build with a new schema rebuilds it), and stop it: the seconds this took, and its documents."""
    index = os.path.abspath(a.index)
    os.makedirs(index, exist_ok=True)
    sock = os.path.join(index, "sock")
    empty = tempfile.mkdtemp(prefix="noweights")
    cmd = [a.index_bin, "serve", "--store", os.path.abspath(a.store), "--index", index, "--threads", "2",
           "--nice", "0", "--weights-dir", empty]
    log = open(os.path.join(index, "build.log"), "w")
    t0 = time.time()
    p = subprocess.Popen(cmd, stdout=log, stderr=log)
    try:
        while time.time() - t0 < 3600:
            st = status(a.index_bin, sock) if os.path.exists(sock) else None
            v = (st or {}).get("vectors") or {}
            if st and st.get("state") == "ready" and st.get("documents") and v.get("chunks") == st.get("documents"):
                row = {"label": a.label, "build_secs": round(time.time() - t0, 1), "documents": st["documents"],
                       "nodes": st.get("nodes")}
                print(json.dumps(row), flush=True)
                return
            time.sleep(0.5)
        raise SystemExit("the index did not become ready in an hour")
    finally:
        p.send_signal(signal.SIGTERM)
        try:
            p.wait(60)
        except subprocess.TimeoutExpired:
            p.kill()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--index-bin", required=True)
    ap.add_argument("--store", required=True)
    ap.add_argument("--index", required=True)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--label", default="run")
    ap.add_argument("--out")
    ap.add_argument("--build", action="store_true", help="only start until ready and stop: build or rebuild the index")
    a = ap.parse_args()
    if a.build:
        return build(a)
    for n in range(a.runs):
        row = one_run(a, n)
        line = json.dumps(row)
        print(line, flush=True)
        if a.out:
            with open(a.out, "a") as f:
                f.write(line + "\n")


if __name__ == "__main__":
    sys.exit(main())

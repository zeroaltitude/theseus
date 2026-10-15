"""Recall's precision, measured without a model (theseus-55hlu).

A finished run of the bench's Theseus arm (`drive.py --arm theseus`, on the
stand-in model of `standin.py`: no key, no spend; `--make` runs one) leaves
the world in its daemon's store. This starts a scratch daemon on a copy of that store, once
for each `[memory] recall_floor` / `recall_subject_bar` setting, and asks
`memory.search` each probe's question: recall's own pipeline, writing
nothing. For each setting and each half of the probes it counts

- on abstention probes, how many admit the same-kind distractor's value;
- on answerable probes, how many still admit the gold value;
- the mean tokens of the pack.

The halves are the probes' numbers, odd (held in: the floor is chosen on
them) and even (held out: it is reported on them once). A note that is the
probe's own question is the store's, not the world's: it is left out of
the counts and the tokens.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tomllib
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import drive  # noqa: E402  (toml_dumps, the daemon's waits)

ANSWERABLE = ("direct", "indirect")


def half_of(probe_id: str) -> str:
    """`in` for an odd probe number, `out` for an even one."""
    digits = "".join(c for c in probe_id if c.isdigit())
    return "in" if int(digits or "0") % 2 else "out"


def own_question(text: str, probe_text: str) -> bool:
    """Whether `text` is the probe's own question, saved as the user's turn."""
    return bool(probe_text) and probe_text[:60] in text


def score_probe(probe: dict, facts: dict, manifest: dict) -> dict:
    """What one search admitted: whether the gold (an answerable probe) or a
    distractor (an abstention) is among the items, and the pack's tokens."""
    items = [i for i in manifest.get("admitted", []) if not own_question(i.get("text") or "", probe["text"])]
    texts = [(i.get("text") or "").lower() for i in items]
    tokens = sum(int(i.get("tokens", 0)) for i in items)
    out = {"probe": probe["id"], "kind": probe["kind"], "bucket": probe["bucket"], "items": len(items),
           "tokens": tokens, "drops": manifest.get("drops", {})}
    # The best a fused score can be: the sources that ranked a hit, each at
    # rank 1 (the recorded ceiling when the floor is on).
    ceiling = (manifest.get("relevance") or {}).get("ceiling") or max(len(manifest.get("sources", {})), 1) / 61.0
    want: list[str] = []
    key = None
    if probe["kind"] in ANSWERABLE and probe.get("fact"):
        want, key = [facts[probe["fact"]]["value"].lower()], "gold"
    elif probe["kind"] == "abstention":
        want, key = [facts[d]["value"].lower() for d in probe.get("distractors", []) if d in facts], "distractor"
    if key:
        out[key] = any(v in t for v in want for t in texts)
        # Each admitted item's share of the ceiling, and whether it holds the value.
        out["shares"] = [{"share": round(float(i.get("fused", 0.0)) / ceiling, 4),
                          "rank": i.get("rank"),
                          "raw": {k: round(float(v.get("score", 0.0)), 4) for k, v in (i.get("sources") or {}).items()},
                          "hit": any(v in (i.get("text") or "").lower() for v in want)} for i in items]
    return out


def summarize(rows: list[dict]) -> dict:
    ans = [r for r in rows if "gold" in r]
    abst = [r for r in rows if "distractor" in r]
    n = len(rows) or 1
    return {
        "probes": len(rows),
        "answerable": len(ans),
        "gold_admitted": sum(r["gold"] for r in ans),
        "abstentions": len(abst),
        "distractor_admitted": sum(r["distractor"] for r in abst),
        "empty_packs": sum(1 for r in rows if r["items"] == 0),
        "mean_tokens": round(sum(r["tokens"] for r in rows) / n, 1),
    }


class Scratch:
    """A scratch daemon on a copy of a run's store."""

    def __init__(self, run: Path, bins: Path, work: Path, memory: dict):
        self.bins, self.work = bins, work
        self.sock = work / "sock"
        self.state = work / "state"
        base = tomllib.loads((run / "daemon" / "config.toml").read_text())
        base["memory"] = {**base.get("memory", {}), **memory}
        self.cfg = work / "config.toml"
        self.cfg.write_text(drive.toml_dumps(base))
        self.proc: subprocess.Popen | None = None
        self.env = dict(drive._env_without(("THESEUS_CONFIG", "THESEUS_STATE_DIR", "THESEUS_SOCKET", "THESEUS_SESSION")),
                        ANTHROPIC_API_KEY="stand-in", XDG_STATE_HOME=str(work / "xdg"))

    def cli(self, *args: str, timeout: float = 60.0) -> subprocess.CompletedProcess:
        return subprocess.run([str(self.bins / "theseus"), "--socket", str(self.sock), *args],
                              capture_output=True, text=True, timeout=timeout, env=self.env)

    def start(self) -> None:
        with (self.work / "theseusd.log").open("w") as log:
            self.proc = subprocess.Popen(
                [str(self.bins / "theseusd"), "--config", str(self.cfg), "--state-dir", str(self.state),
                 "--socket", str(self.sock)],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log, env=self.env)
        t0 = time.monotonic()
        while self.cli("health", timeout=10).returncode != 0:
            if self.proc.poll() is not None or time.monotonic() - t0 > drive.SERVE_WAIT:
                raise RuntimeError(f"theseusd did not serve: see {self.work}/theseusd.log")
            time.sleep(0.1)

    def search(self, query: str, as_of: int | None = None) -> dict:
        extra = ["--as-of", str(as_of)] if as_of is not None else []
        r = self.cli("--json", "memory", "search", *extra, query)
        if r.returncode != 0:
            raise RuntimeError(f"memory search failed: {r.stderr.strip()}")
        return json.loads(r.stdout)

    def wait_indexed(self, probe: str) -> None:
        """Until the tender answers and has read the whole store: a search
        answers `ran` with candidates and the position it holds the store
        through stops moving for three polls."""
        t0, last, steady = time.monotonic(), None, 0
        while time.monotonic() - t0 < 900:
            try:
                m = self.search(probe)
            except RuntimeError:
                time.sleep(1)
                continue
            through = m.get("indexed_through")
            if m.get("outcome") == "ran" and m.get("candidates", 0) > 0 and through == last:
                steady += 1
                if steady >= 3:
                    return
            else:
                steady = 0
            last = through
            time.sleep(2)
        raise RuntimeError("the tender did not index the world in 900 s")

    def stop(self) -> None:
        if self.proc is None:
            return
        try:
            self.cli("shutdown", timeout=10)
            self.proc.wait(drive.STOP_WAIT)
        except (subprocess.TimeoutExpired, OSError):
            self.proc.kill()
            self.proc.wait()
        self.proc = None
        for pid, _ in drive.processes_naming(self.work):
            try:
                import os, signal
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def asked_then(s: "Scratch", probe: dict) -> dict:
    """The search as the probe's turn would have run it: the first search finds
    the probe's own question in the store, and the second asks for only what
    was written before it (`--as-of`)."""
    first = s.search(probe["text"])
    own = [i for i in first.get("admitted", []) if own_question(i.get("text") or "", probe["text"])]
    if not own:
        return first
    return s.search(probe["text"], as_of=min(int(i["position"]) for i in own))


def run_setting(run: Path, bins: Path, work: Path, prog: dict, memory: dict) -> dict:
    facts = {f["id"]: f for f in prog["facts"]}
    s = Scratch(run, bins, work, memory)
    s.start()
    try:
        first = next(p for p in prog["probes"] if p["kind"] in ANSWERABLE and p.get("fact"))
        s.wait_indexed(first["text"])
        rows = [score_probe(p, facts, asked_then(s, p)) for p in prog["probes"]]
    finally:
        s.stop()
    out = {"memory": memory, "all": summarize(rows)}
    for h in ("in", "out"):
        out[h] = summarize([r for r in rows if half_of(r["probe"]) == h])
    out["rows"] = rows
    return out


def make_world(run: Path, bins: Path, seed: int, size: str, overhead: int = 13943) -> None:
    """The bench's Theseus arm on a stand-in model, offline: the progression
    at `seed` and `size`, each turn's work as a model does it and each probe
    answered by a perfect arm (`test_drive.rules_for`), driven through a
    scratch daemon whose store is the world. `run` is the run directory."""
    import test_drive  # the rules a stand-in model follows (a test's helper)
    import standin
    import generate
    prog = generate.build(seed, size, overhead)
    gen = run.parent / (run.name + "-progression")
    prog.save(gen)
    counted = standin.StandIn(test_drive.rules_for(prog), test_drive.REPLY)
    try:
        rc = drive.main(["--arm", "theseus", "--memory-arm", "baseline", "--bin-dir", str(bins),
                         "--progression", str(gen), "--out", str(run), "--turn-timeout", "300",
                         "--api-base", counted.base, "--allow-overhead"])
    finally:
        counted.close()
    if rc != 0:
        raise SystemExit(f"precision: the world's run exited {rc}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--run", required=True, type=Path, help="a finished `drive.py --arm theseus` run directory")
    ap.add_argument("--make", metavar="SEED:SIZE", default=None,
                    help="first run the world into --run (a new directory), e.g. 7:full")
    ap.add_argument("--bin-dir", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--settings", default="0:0",
                    help="comma-separated floor:bar pairs, shares of the best fused score (default 0:0, off)")
    a = ap.parse_args(argv)
    if a.make:
        seed, size = a.make.split(":")
        make_world(a.run, a.bin_dir.resolve(), int(seed), size)
    prog = json.loads((a.run / "progression.json").read_text())
    a.out.mkdir(parents=True, exist_ok=True)
    state = a.out / "state"
    if not state.exists():
        shutil.copytree(a.run / "daemon" / "state", state, ignore=shutil.ignore_patterns("sock", "*.sock", "*.lock"))
    results = []
    for pair in a.settings.split(","):
        floor, bar = (float(x) for x in pair.split(":"))
        work = a.out
        (work / "xdg").mkdir(exist_ok=True)
        r = run_setting(a.run, a.bin_dir.resolve(), work, prog, {"recall_floor": floor, "recall_subject_bar": bar})
        results.append(r)
        print(json.dumps({"floor": floor, "bar": bar, "in": r["in"], "out": r["out"]}), flush=True)
    (a.out / "precision.json").write_text(json.dumps(results, indent=1, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

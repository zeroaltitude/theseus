#!/usr/bin/env python3
"""The thread router's live check (theseus-rha2): forty unaddressed messages
across four threads on a running daemon with a real Jev, reported as top-1 and
the ask rate.

The same forty messages (`crates/theseus-core/fixtures/thread_route_40.json`)
run in theseus-core's `forty_unaddressed_messages_find_their_thread_and_ask_rarely`
against a scripted Jev; this runs them against the daemon you point it at, so
Jev's own answers decide. Run it on a scratch daemon of the build under review
(its own --socket and --state-dir, a fresh state dir, `[judge]` on with the
real key, a real model, `[discord]` off), never on the operator's:

    scripts/thread-route-check.py --socket /tmp/scratch.sock [--dry-run]

It makes the fixture's four threads (so the daemon must have none of those
slugs), opens a session for the stream,
sends each message with `theseus ask --json --session`, and answers any
question the router asks with the right choice (an ask counts against the ask
rate; a question whose right thread is not on offer is answered `home`, and
that message counts as wrong). Only the standard library is used.

Prints, and exits 0 when top-1 is at least 90% and the router asked on at most
10% of the clear messages (all but the ones the fixture marks unclear, which
run in a fresh place of their own). Each message is a model turn: it costs what
forty short turns cost.
"""

import argparse
import json
import re
import subprocess
import sys
import threading
from pathlib import Path

FIXTURE = Path(__file__).resolve().parent.parent / "crates/theseus-core/fixtures/thread_route_40.json"
ASKED = re.compile(r"theseus thread answer (ask_[0-9a-zA-Z]+) ")
KEYED = re.compile(r"\[([0-9a-z]+)\]\s*(.*?)(?=\s{3,}\[|\s*$)")


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--theseus", default="theseus", help="the CLI (default: theseus on PATH)")
    ap.add_argument("--socket", required=True, help="the scratch daemon's socket")
    ap.add_argument("--messages", default=str(FIXTURE), help="the fixture of threads and messages")
    ap.add_argument("--dry-run", action="store_true", help="print what would run and send nothing")
    ap.add_argument("--keep", action="store_true", help="leave the threads (default: finish them)")
    a = ap.parse_args()
    fx = json.load(open(a.messages))
    base = [a.theseus, "--socket", a.socket]

    def th(*args, check=True):
        r = run(base + list(args))
        if check and r.returncode != 0:
            sys.exit(f"{' '.join(args)}: {r.stderr.strip() or r.stdout.strip()}")
        return r

    if a.dry_run:
        for t in fx["threads"]:
            print("thread new", t["slug"], "|", t["title"])
        for i, m in enumerate(fx["messages"], 1):
            print(f"{i:2} [{m['kind']:9}] -> {m['thread']:13} {m['text']}")
        return 0

    for t in fx["threads"]:
        th("thread", "new", t["slug"], "--title", t["title"], "--goal", t["goal"])
        edit = ["thread", "edit", t["slug"]]
        for flag, key in (("--ref", "refs"), ("--repo", "repos"), ("--branch", "branches"),
                          ("--file", "files"), ("--host", "hosts")):
            for v in t.get(key, []):
                edit += [flag, v]
        th(*edit)
    titles = {t["slug"]: t["title"] for t in fx["threads"]}

    def new_place():
        r = th("--json", "rpc", "session.open", "{}")
        return json.loads(r.stdout)["session_id"]

    main_place = new_place()
    right, asks, clear, clear_asks, wrong = 0, 0, 0, 0, []
    for i, m in enumerate(fx["messages"], 1):
        unclear = m["kind"] == "unclear"
        place = new_place() if m.get("cold") else main_place
        p = subprocess.Popen(base + ["ask", "--json", "--session", place, m["text"]],
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        asked = []

        def watch():
            nonlocal asks
            options = {}
            for line in p.stderr:
                if "[1]" in line or "[h]ome" in line:
                    options = {k: v.strip() for k, v in KEYED.findall(line.strip())}
                found = ASKED.search(line)
                if found:
                    asked.append(found.group(1))
                    want = titles[m["thread"]]
                    key = next((k for k, v in options.items() if v == want), "home")
                    th("thread", "answer", found.group(1), key)

        w = threading.Thread(target=watch, daemon=True)
        w.start()
        out, _ = p.communicate(timeout=900)
        w.join(timeout=5)
        if p.returncode not in (0, 9):
            sys.exit(f"message {i} failed ({p.returncode}): {m['text']}")
        res = json.loads(out)
        tag = res.get("thread") or {}
        landed = tag.get("slug") or "home"
        asks += len(asked)
        if not unclear:
            clear += 1
            clear_asks += len(asked)
        ok = landed == m["thread"]
        right += ok
        mark = "ok " if ok else "BAD"
        how = tag.get("how", "-")
        print(f"{i:2} {mark} {m['kind']:9} -> {landed:13} ({how}{', asked' if asked else ''}) {m['text']}")
        if not ok:
            wrong.append((m["text"], landed, m["thread"]))

    if not a.keep:
        for t in fx["threads"]:
            th("thread", "done", t["slug"], check=False)
    n = len(fx["messages"])
    top1 = right / n
    rate = clear_asks / clear
    print(f"\ntop-1 {right}/{n} = {top1:.0%}; asks {asks} ({clear_asks} of {clear} clear = {rate:.1%})")
    for text, got, want in wrong:
        print(f"  wrong: {text!r} went to {got}, was for {want}")
    return 0 if top1 >= 0.90 and rate <= 0.10 else 1


if __name__ == "__main__":
    sys.exit(main())

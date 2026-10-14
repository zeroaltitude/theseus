#!/usr/bin/env python3
"""F10's threads check (theseus-j0yh): a person with two threads switches between
them by `/t` in `theseus watch --interactive`, and each thread's answers stay in
its own session.

A scratch daemon on the scripted stand-in model ($0, `arm_theseus.Theseus`), two
threads (`tide-log`, `moon-table`) each opened by one `theseus ask --thread`, and
one interactive watch that starts on `tide-log` (`THESEUS_THREAD`, the export
`theseus thread switch` prints), asks, switches by `/t moon-table`, asks, and
switches back by `/t tide-log`. It passes when

- each reply reached the terminal, in order,
- the status line named each thread it was switched to,
- `tide-log`'s session holds its own questions and replies and none of the moon
  table's, and the moon table's the converse.

    python3 scripts/f10/threads_check.py --bin-dir target/debug --out /tmp/f10-threads

Prints `threads: PASS` or `threads: FAIL (why)` and writes `threads.json` and the
watch's `transcript.txt` into the output directory. Standard library only;
`check()` is a pure function of the transcript and the two sessions' texts
(tested in test_f10.py).
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import steps  # noqa: E402
from arm_theseus import Theseus  # noqa: E402
from driver import wait_until  # noqa: E402

P = steps.PROMPTS
R = steps.T_REPLIES

TIDE_OWN = [P["T.tide.first"], R["T.tide.first"], P["T.tide.again"], R["T.tide.again"]]
MOON_OWN = [P["T.moon.first"], R["T.moon.first"], P["T.moon.again"], R["T.moon.again"]]


def check(transcript, tide, moon):
    """The failures, as words: none when the threads kept their answers apart.
    `transcript` is the watch's output; `tide` and `moon` are the texts of each
    thread's session."""
    fails = []
    at = [transcript.find(R["T.tide.again"]), transcript.find(R["T.moon.again"])]
    if min(at) < 0:
        fails.append("a reply never reached the terminal")
    elif at[0] > at[1]:
        fails.append("the replies arrived out of order")
    for status in ("[Tide log", "[Moon table"):
        if status not in transcript:
            fails.append(f"the status line never said {status}")
    for words, mine, other, name in ((tide, TIDE_OWN, MOON_OWN, "tide-log"), (moon, MOON_OWN, TIDE_OWN, "moon-table")):
        lacks = [w for w in mine if not any(w in x for x in words)]
        holds = [w for w in other if any(w in x for x in words)]
        if lacks:
            fails.append(f"{name}'s session lacks its own words: {lacks}")
        if holds:
            fails.append(f"{name}'s session holds the other thread's words: {holds}")
    return fails


class Scratch:
    """What `Theseus.setup` needs of a driver's context."""

    def __init__(self, root):
        self.root = Path(root)
        self.project = self.root / "project"
        self.meta = {}
        shutil.copytree(HERE / "project", self.project)


def texts(arm, slug):
    r = arm.cli("--json", "thread", "show", slug)
    sid = json.loads(r.stdout)["thread"].get("session_id")
    if not sid:
        return []
    nodes = json.loads(arm.cli("--json", "history", sid).stdout)
    nodes = nodes.get("nodes", nodes) if isinstance(nodes, dict) else nodes
    return [n.get("text") or "" for n in nodes]


def run(bin_dir, out):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    ctx = Scratch(tempfile.mkdtemp(prefix="f10-threads-"))
    arm = Theseus(bin_dir)
    result = {"passed": False, "fails": []}
    try:
        env = arm.setup(ctx)
        for slug, title in (("tide-log", "Tide log"), ("moon-table", "Moon table")):
            r = arm.cli("thread", "new", slug, "--title", title)
            if r.returncode != 0:
                raise RuntimeError(f"thread new {slug}: {r.stderr}")
        for slug, first in (("tide-log", P["T.tide.first"]), ("moon-table", P["T.moon.first"])):
            r = arm.cli("ask", "--no-stream", "--thread", slug, first)
            if r.returncode != 0:
                raise RuntimeError(f"ask --thread {slug}: {r.stderr}")
        wenv = dict(os.environ, **env, THESEUS_THREAD="tide-log", COLUMNS="100")
        w = subprocess.Popen([arm.bin / "theseus", "--socket", arm.sock, "watch", "--interactive", "--no-herdr"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, env=wenv)
        buf = []
        t = threading.Thread(target=lambda: [buf.append(line) for line in w.stdout], daemon=True)
        t.start()

        def say(line, expect):
            w.stdin.write(line + "\n")
            w.stdin.flush()
            if not wait_until(lambda: expect in "".join(buf), 30):
                raise RuntimeError(f"no `{expect}` after `{line}`; saw:\n{''.join(buf)}")

        say(P["T.tide.again"], R["T.tide.again"])
        say("/t moon-table", "[Moon table")
        say(P["T.moon.again"], R["T.moon.again"])
        say("/t tide-log", "[Tide log")
        w.stdin.close()
        try:
            w.wait(10)
        except subprocess.TimeoutExpired:
            w.kill()
        t.join(2)
        transcript = "".join(buf)
        (out / "transcript.txt").write_text(transcript)
        result["fails"] = check(transcript, texts(arm, "tide-log"), texts(arm, "moon-table"))
        result["passed"] = not result["fails"]
    except Exception as e:  # noqa: BLE001
        result["fails"].append(f"the run failed: {e}")
    finally:
        arm.teardown(ctx)
        (out / "threads.json").write_text(json.dumps(result, indent=1))
        shutil.rmtree(ctx.root, ignore_errors=True)
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin-dir", required=True)
    ap.add_argument("--out", default="/tmp/f10-threads")
    a = ap.parse_args()
    r = run(a.bin_dir, a.out)
    if r["passed"]:
        print("threads: PASS")
        return 0
    print("threads: FAIL (" + "; ".join(r["fails"]) + ")")
    return 1


if __name__ == "__main__":
    sys.exit(main())

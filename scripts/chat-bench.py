#!/usr/bin/env python3
"""The chat's FAST row (theseus-cyha): launch to a ready prompt, the banner,
a keystroke's echo, and the resident set, measured on the real `theseus-tui chat`
on a pseudo-terminal against a real `theseusd` whose store holds N parked sessions
(`theseus-sim synth-store`). Standard library only.

    python3 scripts/chat-bench.py --bin-dir target/release-thin [--sessions 1000] [--runs 20]

The bars: a ready prompt (the `›` on the screen) at most 100 ms at the 90th
percentile, a keystroke's echo at most 16 ms at the 99th, resident at most 20 MB.
The echo is measured on an idle chat: while a turn runs the dock's clock draws
at 10 Hz, and a key that lands within 33 ms of a draw (the 30-frames-a-second
bound) waits for the next frame.
"""

import argparse
import fcntl
import json
import os
import pty
import select
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "f10"))

import arm_theseus  # noqa: E402  (its daemon config, on the stand-in model)


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, round(p / 100 * (len(xs) - 1)))]


def rss_kb(pid):
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except OSError:
        pass
    return 0


class Run:
    """One chat on a pty: reads of its output, with times."""

    def __init__(self, tui, sock, cwd, env):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        self.t0 = time.monotonic()
        self.child = subprocess.Popen(
            [tui, "--socket", sock, "chat"], stdin=slave, stdout=slave, stderr=subprocess.DEVNULL,
            cwd=cwd, env=env, start_new_session=True)
        os.close(slave)
        self.out = b""

    def read(self, until, timeout=10.0):
        """Read until `until(out)`; the seconds since the start, or None."""
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if until(self.out):
                return time.monotonic() - self.t0
            r, _, _ = select.select([self.master], [], [], 0.002)
            if r:
                try:
                    self.out += os.read(self.master, 65536)
                except OSError:
                    return None
        return None

    def close(self):
        try:
            os.write(self.master, b"\x03\x03\x03")
            self.child.wait(5)
        except (OSError, subprocess.TimeoutExpired):
            self.child.kill()
        os.close(self.master)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin-dir", required=True)
    ap.add_argument("--sessions", type=int, default=1000)
    ap.add_argument("--runs", type=int, default=20)
    ap.add_argument("--keys", type=int, default=100)
    a = ap.parse_args()
    bins = Path(a.bin_dir).resolve()
    root = Path(tempfile.mkdtemp(prefix="chat-bench-"))
    state, sock, project = root / "state", root / "t.sock", root / "project"
    project.mkdir()
    cfg = root / "theseus.toml"
    cfg.write_text(arm_theseus.config_text(state, sock, project, "http://127.0.0.1:9"))
    env = dict(os.environ, PATH=f"{bins}:{os.environ.get('PATH', '')}", TERM="xterm-256color",
               THESEUS_SOCKET=str(sock), THESEUS_CONFIG=str(cfg), XDG_STATE_HOME=str(root / "xdg"),
               F10_STANDIN_KEY="bench", HOME=str(root))
    subprocess.run([bins / "theseus-sim", "synth-store", "--dir", state / "store",
                    "--sessions", str(a.sessions)], check=True, stdout=subprocess.DEVNULL)
    daemon = subprocess.Popen([bins / "theseusd", "--config", cfg, "--socket", sock, "--state-dir", state],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              start_new_session=True)
    try:
        end = time.monotonic() + 60
        while subprocess.run([bins / "theseus", "--socket", sock, "health"], env=env,
                             capture_output=True).returncode != 0:
            if time.monotonic() > end:
                sys.exit("the daemon did not answer")
            time.sleep(0.1)
        prompt, banner, rss, echo = [], [], [], []
        for _ in range(a.runs):
            r = Run(bins / "theseus-tui", str(sock), project, env)
            t = r.read(lambda o: "›".encode() in o)
            b = r.read(lambda o: b"? help" in o)
            if t is None or b is None:
                sys.exit(f"no prompt or banner: {r.out!r}")
            prompt.append(t * 1000)
            banner.append(b * 1000)
            rss.append(rss_kb(r.child.pid))
            r.close()
        r = Run(bins / "theseus-tui", str(sock), project, env)
        r.read(lambda o: b"? help" in o)
        time.sleep(0.3)
        for i in range(a.keys):
            ch = bytes([ord("a") + i % 26])
            before = len(r.out)
            t0 = time.monotonic()
            os.write(r.master, ch)
            got = r.read(lambda o: ch in o[before:], timeout=2.0)
            if got is not None:
                echo.append((time.monotonic() - t0) * 1000)
            time.sleep(0.04)
        rss.append(rss_kb(r.child.pid))
        r.close()
        res = {
            "sessions": a.sessions, "runs": a.runs,
            "prompt_ms": {"p50": round(pct(prompt, 50), 1), "p90": round(pct(prompt, 90), 1), "max": round(max(prompt), 1)},
            "banner_ms": {"p50": round(pct(banner, 50), 1), "p90": round(pct(banner, 90), 1)},
            "echo_ms": {"p50": round(pct(echo, 50), 2), "p99": round(pct(echo, 99), 2), "n": len(echo)},
            "rss_mb_max": round(max(rss) / 1024, 1),
            "bars": {"prompt_p90_le_100": pct(prompt, 90) <= 100, "echo_p99_le_16": pct(echo, 99) <= 16,
                     "rss_le_20mb": max(rss) / 1024 <= 20},
        }
        print(json.dumps(res, indent=1))
        return 0 if all(res["bars"].values()) else 1
    finally:
        subprocess.run([bins / "theseus", "--socket", sock, "shutdown"], env=env, capture_output=True)
        try:
            daemon.wait(15)
        except subprocess.TimeoutExpired:
            os.killpg(daemon.pid, signal.SIGKILL)


if __name__ == "__main__":
    sys.exit(main())

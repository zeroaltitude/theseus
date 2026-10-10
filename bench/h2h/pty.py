"""The head-to-head bench's terminal driver (theseus-7gir.13): an interactive surface on a pseudo-terminal, typed
at as a person would, its output read on a thread of its own and stamped as it arrives.

- `Transcript`: what a surface wrote, as plain text. Escape sequences are dropped, except that a cursor-forward
  (`ESC[nC`) is n spaces and an absolute move (`ESC[r;cH`) a line break, so words a full-screen surface draws with
  moves between them stay apart. Every byte's arrival time is kept, so `stamp(marker)` says when a word first
  appeared on screen, whenever it is asked. Pure: `feed(bytes, ns)` and nothing else.
- `Screen`: the same output on a 120x40 grid, the cursor's moves applied, for a surface that draws a word in two
  places (a full-screen TUI redraws a line by cursor moves, so its words are split in the transcript). A marker it is
  told to watch is stamped by the read after which the grid first holds it.
- `Pty`: a child on a 120x40 pseudo-terminal, `type`, `key` (`enter`, `ctrl-c`, ...), and `wait_for(marker)`. It
  answers the terminal queries a real terminal would (cursor position, device attributes, background colour), so a
  surface that asks is not left waiting on the bench.
- `sample_tree(pids)`: RSS and CPU ticks (utime + stime) of processes and all their descendants, from `/proc`.

Standard library only. Every time is `time.monotonic_ns()`, CLOCK_MONOTONIC, the stand-in's clock too.
"""

from __future__ import annotations

import fcntl
import os
import re
import signal
import struct
import subprocess
import termios
import threading
import time
from pathlib import Path

COLS, ROWS = 120, 40

KEYS = {"enter": b"\r", "ctrl-c": b"\x03", "ctrl-d": b"\x04", "esc": b"\x1b", "tab": b"\t", "backspace": b"\x7f",
        "up": b"\x1b[A", "down": b"\x1b[B", "right": b"\x1b[C", "left": b"\x1b[D"}

# One escape sequence: CSI (with its parameters and final byte), OSC (to BEL or ST), DCS/APC/PM/SOS strings
# (to ST), or a two-byte escape. `_PARTIAL` is an escape cut off at the end of a read: kept for the next one.
_SEQ = re.compile(rb"\x1b(?:\[([0-9;?<>=!]*)[ -/]*([@-~])|\][^\x07\x1b]*(?:\x07|\x1b\\)|[P_^X][^\x1b]*\x1b\\|[ -/]*[0-~])")
_PARTIAL = re.compile(rb"\x1b(?:\[[0-9;?<>=! -/]*|\][^\x07\x1b]*|[P_^X][^\x1b]*|[ -/]*)?$")
_CTRL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")


class Transcript:
    """A surface's output as text, with when each part of it arrived."""

    def __init__(self) -> None:
        self.text = ""
        self.ends: list[tuple[int, int]] = []  # (text length after a feed, its arrival ns)
        self._tail = b""

    def feed(self, data: bytes, ns: int) -> str:
        """Add one read's bytes, arrived at `ns`; returns the text they added."""
        data = self._tail + data
        cut = _PARTIAL.search(data)
        self._tail, data = (data[cut.start():], data[:cut.start()]) if cut else (b"", data)
        added = strip(data)
        if added:
            self.text += added
            self.ends.append((len(self.text), ns))
        return added

    def stamp(self, marker: str, after: int = 0) -> int | None:
        """When `marker` first stood whole in the text at or past offset `after`: the arrival of the read that
        completed it. None while it has not appeared."""
        i = self.text.find(marker, after)
        if i < 0:
            return None
        end = i + len(marker)
        for length, ns in self.ends:
            if length >= end:
                return ns
        return None


def strip(data: bytes) -> str:
    """Bytes a terminal was sent, as their text: ESC[nC as n spaces, an absolute move as a line break, every other
    escape and control character dropped. A move to a column (ESC[nG) is one space, any move to another row a
    line break."""
    out = []
    pos = 0
    for m in _SEQ.finditer(data):
        out.append(data[pos:m.start()])
        params, final = m.group(1), m.group(2)
        if final == b"C":
            out.append(b" " * max(1, int((params or b"1").split(b";")[0] or 1)))
        elif final == b"G":  # to a column: a gap, as far as text goes
            out.append(b" ")
        elif final and final in b"HfdABEF":  # to another row
            out.append(b"\n")
        pos = m.end()
    out.append(data[pos:])
    text = b"".join(out).decode("utf-8", "replace").replace("\r\n", "\n").replace("\r", "\n")
    return _CTRL.sub("", text)


class Screen:
    """A terminal's grid: text put where the cursor is, with the moves, erases and scrolls a TUI uses. Enough of
    a VT100 for words to stand where a person sees them; colours and modes are ignored."""

    def __init__(self, cols: int = COLS, rows: int = ROWS) -> None:
        self.cols, self.rows = cols, rows
        self.grid = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0
        self._tail = b""
        self.watched: dict[str, int | None] = {}

    def watch(self, marker: str) -> None:
        if marker not in self.watched:
            self.watched[marker] = None

    def text(self) -> str:
        return "\n".join("".join(row).rstrip() for row in self.grid)

    def _scroll(self) -> None:
        self.grid.pop(0)
        self.grid.append([" "] * self.cols)
        self.r = self.rows - 1

    def _put(self, ch: str) -> None:
        if ch == "\r":
            self.c = 0
        elif ch == "\n":
            self.r += 1
            if self.r >= self.rows:
                self._scroll()
        elif ch == "\b":
            self.c = max(0, self.c - 1)
        elif ch >= " ":
            if self.c >= self.cols:
                self.c = 0
                self.r += 1
                if self.r >= self.rows:
                    self._scroll()
            self.grid[self.r][self.c] = ch
            self.c += 1

    def _csi(self, params: bytes, final: bytes) -> None:
        nums = [int(x) if x.isdigit() else 0 for x in params.lstrip(b"?<>=!").split(b";")] if params else []
        n = (nums[0] if nums else 0) or 1
        if final in (b"H", b"f"):
            self.r = min(self.rows - 1, max(0, (nums[0] if nums else 1) - 1 if nums and nums[0] else 0))
            self.c = min(self.cols - 1, max(0, (nums[1] - 1) if len(nums) > 1 and nums[1] else 0))
        elif final == b"A":
            self.r = max(0, self.r - n)
        elif final in (b"B", b"E"):
            self.r = min(self.rows - 1, self.r + n)
            if final == b"E":
                self.c = 0
        elif final == b"F":
            self.r, self.c = max(0, self.r - n), 0
        elif final == b"C":
            self.c = min(self.cols - 1, self.c + n)
        elif final == b"D":
            self.c = max(0, self.c - n)
        elif final == b"G":
            self.c = min(self.cols - 1, n - 1)
        elif final == b"d":
            self.r = min(self.rows - 1, n - 1)
        elif final == b"K":
            mode = nums[0] if nums else 0
            lo, hi = {0: (self.c, self.cols), 1: (0, self.c + 1)}.get(mode, (0, self.cols))
            for i in range(lo, min(hi, self.cols)):
                self.grid[self.r][i] = " "
        elif final == b"J":
            mode = nums[0] if nums else 0
            if mode in (2, 3):
                self.grid = [[" "] * self.cols for _ in range(self.rows)]
            elif mode == 0:
                self._csi(b"0", b"K")
                for row in range(self.r + 1, self.rows):
                    self.grid[row] = [" "] * self.cols
        elif final in (b"h", b"l") and params.startswith(b"?") and b"1049" in params:
            self.grid = [[" "] * self.cols for _ in range(self.rows)]
            self.r = self.c = 0

    def feed(self, data: bytes, ns: int) -> None:
        """Apply one read's bytes, arrived at `ns`, and stamp each watched marker the grid now holds first."""
        data = self._tail + data
        cut = _PARTIAL.search(data)
        self._tail, data = (data[cut.start():], data[:cut.start()]) if cut else (b"", data)
        pos = 0
        for m in _SEQ.finditer(data):
            for ch in data[pos:m.start()].decode("utf-8", "replace"):
                self._put(ch)
            if m.group(2) is not None:
                self._csi(m.group(1) or b"", m.group(2))
            pos = m.end()
        for ch in data[pos:].decode("utf-8", "replace"):
            self._put(ch)
        waiting = [k for k, v in self.watched.items() if v is None]
        if waiting:
            text = self.text()
            for k in waiting:
                if k in text:
                    self.watched[k] = ns

    def stamp(self, marker: str) -> int | None:
        return self.watched.get(marker)


# The queries a real terminal answers, and its answers (a 120x40 dark terminal).
_QUERIES = [(b"\x1b[6n", b"\x1b[1;1R"), (b"\x1b[c", b"\x1b[?62;22c"), (b"\x1b[0c", b"\x1b[?62;22c"),
            (b"\x1b[>c", b"\x1b[>0;10;1c"), (b"\x1b[?u", b"\x1b[?0u"), (b"\x1b]10;?", b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
            (b"\x1b]11;?", b"\x1b]11;rgb:0000/0000/0000\x1b\\"), (b"\x1b[18t", b"\x1b[8;40;120t"),
            (b"\x1b[14t", b"\x1b[4;800;1200t")]


class Pty:
    """One interactive surface on a pseudo-terminal of COLS x ROWS."""

    def __init__(self, argv: list[str], env: dict[str, str] | None = None, cwd: str | None = None,
                 log: Path | None = None) -> None:
        self.master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        env = dict(env if env is not None else os.environ)
        env.setdefault("TERM", "xterm-256color")
        env["COLUMNS"], env["LINES"] = str(COLS), str(ROWS)
        self.screen = Transcript()
        self.grid = Screen()
        self.raw = open(log, "w") if log else None  # a recording: one line per read, "<ns> <hex>"
        self.lock = threading.Condition()
        self.spawn_ns = time.monotonic_ns()
        self.proc = subprocess.Popen(argv, stdin=slave, stdout=slave, stderr=slave, env=env, cwd=cwd,
                                     start_new_session=True, close_fds=True,
                                     preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
        os.close(slave)
        self.first_byte_ns: int | None = None
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    @property
    def pid(self) -> int:
        return self.proc.pid

    def _read(self) -> None:
        while True:
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            ns = time.monotonic_ns()
            for q, answer in _QUERIES:
                if q in data:
                    try:
                        os.write(self.master, answer)
                    except OSError:
                        pass
            if self.raw:
                self.raw.write(f"{ns} {data.hex()}\n")
            with self.lock:
                if self.first_byte_ns is None:
                    self.first_byte_ns = ns
                self.screen.feed(data, ns)
                self.grid.feed(data, ns)
                self.lock.notify_all()
        with self.lock:
            self.lock.notify_all()

    def write(self, data: bytes) -> int:
        """Send bytes as typed; returns the ns just before they went."""
        ns = time.monotonic_ns()
        os.write(self.master, data)
        return ns

    def type(self, text: str, per_char_ms: float = 0) -> int:
        """Type text, all at once or a character at a time; returns when the first byte went."""
        if not per_char_ms:
            return self.write(text.encode())
        first = None
        for ch in text:
            ns = self.write(ch.encode())
            first = first or ns
            time.sleep(per_char_ms / 1000)
        return first or time.monotonic_ns()

    def key(self, name: str) -> int:
        return self.write(KEYS[name])

    def wait_for(self, marker: str, timeout: float = 30.0, after: int = 0) -> int | None:
        """The ns `marker` first appeared on screen (past text offset `after`), waiting up to `timeout` seconds for
        it; None if it never did."""
        deadline = time.monotonic() + timeout
        with self.lock:
            if not after:
                self.grid.watch(marker)
            while True:
                ns = self.stamp(marker, after)
                if ns is not None:
                    return ns
                left = deadline - time.monotonic()
                if left <= 0 or (not self.reader.is_alive()):
                    return None
                self.lock.wait(min(left, 0.5))

    def watch(self, *markers: str) -> None:
        """Stamp these markers on the grid from now on (a marker the transcript splits is still seen whole)."""
        with self.lock:
            for m in markers:
                self.grid.watch(m)

    def stamp(self, marker: str, after: int = 0) -> int | None:
        """When `marker` first appeared: in the transcript, else (unless `after`) on the grid, the earlier."""
        stamps = [self.screen.stamp(marker, after)]
        if not after:
            stamps.append(self.grid.stamp(marker))
        got = [ns for ns in stamps if ns is not None]
        return min(got) if got else None

    def mark(self) -> int:
        """The text offset now: pass it as `after` to wait only for what comes next."""
        with self.lock:
            return len(self.screen.text)

    def close(self, timeout: float = 5.0) -> int | None:
        """End the surface: SIGTERM to its group, then SIGKILL; returns its exit code."""
        if self.proc.poll() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(timeout)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait()
        try:
            os.close(self.master)
        except OSError:
            pass
        self.reader.join(2)
        if self.raw:
            self.raw.close()
        return self.proc.returncode


def replay(path: Path) -> Transcript:
    """A recording `Pty(log=...)` wrote, fed again into a Transcript."""
    t = Transcript()
    for line in Path(path).read_text().splitlines():
        if line.strip():
            ns, hexed = line.split(" ", 1)
            t.feed(bytes.fromhex(hexed.strip()), int(ns))
    return t


# ------------------------------------------------------------------ /proc

CLK_TCK = os.sysconf("SC_CLK_TCK")
PAGE_KB = os.sysconf("SC_PAGE_SIZE") // 1024


def _stat(pid: int) -> tuple[int, int, int] | None:
    """(ppid, utime + stime ticks, rss pages) of a process, None when it is gone."""
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    fields = raw[raw.rindex(")") + 2:].split()  # past the command, which may hold spaces
    return int(fields[1]), int(fields[11]) + int(fields[12]), int(fields[21])


def descendants(roots: list[int]) -> set[int]:
    """The roots and every process under them."""
    parent = {}
    for d in os.listdir("/proc"):
        if d.isdigit() and (s := _stat(int(d))):
            parent[int(d)] = s[0]
    out = {p for p in roots if p in parent}
    grew = True
    while grew:
        grew = False
        for pid, pp in parent.items():
            if pp in out and pid not in out:
                out.add(pid)
                grew = True
    return out


def sample_tree(pids: list[int]) -> dict[str, int]:
    """RSS (KB) and CPU ticks (utime + stime, at CLK_TCK a second) summed over `pids` and their descendants, now."""
    rss = cpu = n = 0
    for pid in descendants([p for p in pids if p]):
        s = _stat(pid)
        if s:
            cpu += s[1]
            rss += s[2] * PAGE_KB
            n += 1
    return {"ns": time.monotonic_ns(), "rss_kb": rss, "cpu_ticks": cpu, "procs": n, "clk_tck": CLK_TCK}

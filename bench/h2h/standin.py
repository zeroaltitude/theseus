"""The head-to-head bench's stand-in model (theseus-7gir.13): its rules for each harness, its process, and its log.

Both arms talk to one `theseus-sim fake-model`, which speaks the Anthropic Messages API, streams its answers with a
fixed first-byte delay and chunking, and logs every request with CLOCK_MONOTONIC stamps (arrival, first byte, last
byte). The rules give both harnesses the same work, each in its own tools' words:

    prompt word   steps                                         rows
    h2h-hello     text                                          T1, T2, T3, T5, T6, T7, T8
    h2h-read      read notes.txt, then text                     T4 (read)
    h2h-shell     run `cat notes.txt`, then text                T4 (shell)
    h2h-task3     read, edit a line, run `wc -l`, then text     task3

Claude Code's tools are Read {file_path}, Edit {file_path, old_string, new_string} and Bash {command}; Theseus's
are fs_read {path}, fs_edit {path, old_string, new_string} and proc_run {argv} (crates/theseus-tools/src/fs.rs and
proc.rs). Paths are absolute, so both read the same file wherever they run. Every prompt carries `marker=<word>`;
each reply's text is `<kind>-<marker> ... end-<marker>`, so the screen says which turn's reply it is and when it
began and ended.

Standard library only.
"""

from __future__ import annotations

import json
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

NOTES = "alpha line\nbeta line\ngamma line\n"

TOOLS = {
    "claude-code": {
        "read": lambda f: {"name": "Read", "input": {"file_path": f}},
        "edit": lambda f: {"name": "Edit", "input": {"file_path": f, "old_string": "beta line",
                                                    "new_string": "beta line edited {marker}"}},
        "shell": lambda f: {"name": "Bash", "input": {"command": f"cat {f}"}},
        "count": lambda f: {"name": "Bash", "input": {"command": f"wc -l {f}"}},
    },
    "theseus": {
        "read": lambda f: {"name": "fs_read", "input": {"path": f}},
        "edit": lambda f: {"name": "fs_edit", "input": {"path": f, "old_string": "beta line",
                                                       "new_string": "beta line edited {marker}"}},
        "shell": lambda f: {"name": "proc_run", "input": {"argv": ["cat", f]}},
        "count": lambda f: {"name": "proc_run", "input": {"argv": ["wc", "-l", f]}},
    },
}

# Each prompt word's tool steps (kinds for T4's labels: "count" is a shell command), then its reply's word.
WORK = {
    "h2h-hello": ([], "ok"),
    "h2h-read": (["read"], "read"),
    "h2h-shell": (["shell"], "shell"),
    "h2h-task3": (["read", "edit", "count"], "task"),
}
KIND = {"read": "read", "edit": "edit", "shell": "shell", "count": "shell"}


def reply_text(word: str) -> str:
    return f"{word}-{{marker}} the stand-in answers in a few short words end-{{marker}}"


def rules(arm: str, notes: Path, ttfb_ms: int = 300, chunks: int = 8, chunk_ms: int = 25) -> list[dict[str, Any]]:
    """The rules file's content for one arm: the same steps, in that arm's tools."""
    tools = TOOLS[arm]
    out = []
    for when, (calls, word) in WORK.items():
        steps = [{"calls": [tools[c](str(notes))]} for c in calls] + [{"text": reply_text(word)}]
        out.append({"when": when, "steps": steps, "ttfb_ms": ttfb_ms, "chunks": chunks, "chunk_ms": chunk_ms})
    out.append({"when": "", "steps": [{"text": "fallback-{marker} end-{marker}"}], "ttfb_ms": ttfb_ms,
                "chunks": chunks, "chunk_ms": chunk_ms})
    return out


def step_kinds(prompt_word: str) -> list[str]:
    """The tool kind each step of a prompt word's turn asks for (T4's labels): read, edit, or shell."""
    return [KIND[c] for c in WORK[prompt_word][0]]


def write_rules(path: Path, arm: str, notes: Path, **timing: int) -> Path:
    path.write_text(json.dumps(rules(arm, notes, **timing), indent=1))
    return path


def reset_work(work: Path) -> Path:
    """The work directory as every run starts it: notes.txt with three lines. Returns the file."""
    work.mkdir(parents=True, exist_ok=True)
    notes = work / "notes.txt"
    notes.write_text(NOTES)
    return notes


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class StandIn:
    """One `theseus-sim fake-model` process, listening on 127.0.0.1."""

    def __init__(self, sim: str, rules_path: Path, log: Path, port: int | None = None, ttfb_ms: int = 300,
                 chunks: int = 8, chunk_ms: int = 25, timeout: float = 15.0) -> None:
        self.port = port or free_port()
        self.log = log
        argv = [sim, "fake-model", "--addr", f"127.0.0.1:{self.port}", "--rules", str(rules_path), "--log", str(log),
                "--ttfb-ms", str(ttfb_ms), "--chunks", str(chunks), "--chunk-ms", str(chunk_ms)]
        self.proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.base_url = self._listening(timeout)

    def _listening(self, timeout: float) -> str:
        found: list[str] = []
        done = threading.Event()

        def read() -> None:
            for line in self.proc.stdout:
                if line.startswith("fake model on "):
                    found.append(line.split("fake model on ", 1)[1].strip())
                    done.set()
            done.set()

        threading.Thread(target=read, daemon=True).start()
        if not done.wait(timeout) or not found:
            self.stop()
            raise RuntimeError(f"the stand-in did not say it was listening: {self.proc.stderr.read()[:400]}")
        return found[0]

    @property
    def pid(self) -> int:
        return self.proc.pid

    def entries(self) -> list[dict[str, Any]]:
        return read_log(self.log)

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


# The log's keys, with what a line that lacks one reads as. The times are required: a line without its arrival is
# no request this bench can place.
DEFAULTS: dict[str, Any] = {"conn": None, "conn_req": 0, "path": "", "first_byte_ns": None, "last_byte_ns": None,
                            "req_bytes": None, "resp_bytes": None, "model": None, "tools": [], "side": False,
                            "rule": None, "step": 0, "tool_results": 0, "opening": "", "chunks": None}


def read_log(path: Path) -> list[dict[str, Any]]:
    """The stand-in's log as a list of requests in arrival order. A torn last line (the stand-in still writing) and
    a line without `arrival_ns` are skipped; missing optional keys take their defaults."""
    out = []
    try:
        lines = Path(path).read_text().splitlines()
    except FileNotFoundError:
        return out
    for line in lines:
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(e, dict) or not isinstance(e.get("arrival_ns"), int):
            continue
        out.append({**DEFAULTS, **e})
    out.sort(key=lambda e: e["arrival_ns"])
    for i, e in enumerate(out):
        e.setdefault("seq", i)
    return out


def wait_requests(log: Path, pred, n: int = 1, timeout: float = 30.0) -> list[dict[str, Any]]:
    """Wait until `n` logged requests satisfy `pred` and their responses are done (`last_byte_ns`); returns them."""
    deadline = time.monotonic() + timeout
    while True:
        got = [e for e in read_log(log) if pred(e) and e.get("last_byte_ns")]
        if len(got) >= n or time.monotonic() > deadline:
            return got
        time.sleep(0.02)

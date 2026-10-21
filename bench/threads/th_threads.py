#!/usr/bin/env python3
"""threadbench's threads arms (theseus-l7z1): the recall bench's scratch-daemon driver, taught the threads product.

    A1   threads, natural addressing: every message goes to the home session unaddressed; the router places it
    A1n  A1 with `[prefix] enabled = false`: what the prefix library buys on top of threads
    A1p  A1 with a prompt layer per persona: NOT MEASURABLE (the daemon has no switch for that layout)
    A2   threads, explicit addressing: a stream's message goes out with `ask --thread <its slug>`
    A1o  A1 with the ontology on (`+onto`, theseus-mqqc2): Jev files each message (`onto.v1`), a project answer
         attaches the thread, and recall's category boost is on. The other threads arms turn all three off, so
         the pair differs by the ontology and nothing else.

All four measurable arms share the scratch config (the bench profile, the five persona cards, the prefix library
at its default) and the simulated owner (`th_owner.py`). Each message is one `theseus ask`; its answer is read
from what the daemon says, never from the message's label:

- where it ran: the reply's `thread` tag (`TurnSubmitResult.thread`: slug, how, confidence), none when the place
  had no live thread to choose from, so it ran at home;
- whether the router asked: a `thread.asked` the CLI prints on stderr while the turn waits, which the owner answers
  with `theseus thread answer` (the CLI's own line, `thread/ask.rs`);
- the engaged persona and the prefix library's pick: the turn's `persona.engaged` and `prefix.picked` ledger rows.

`routing.jsonl` is one record per message, in `th_score.py`'s shape, with the extras it reads when they are there.

A prefix-track `reopen` is not driven: a thread goes cold when its cache window passes (wall-clock time, `[cache]`),
which nothing outside the daemon can move without waiting hours, so the message is sent as it is and `run.json`
says so (`reopen_not_driven`).

Standard library only.
"""

from __future__ import annotations

import json
import queue
import signal
import subprocess
import threading
import time
import tomllib
from pathlib import Path

import drive as rd  # noqa: E402  (the recall bench's drivers; th_drive has put it on the path)
import progression as pg  # noqa: E402
import th_decisions
import th_owner as owner_mod

HOME = owner_mod.HOME
ROUTING = "routing.jsonl"
# The arms' scratch-config differences from the common one: a dotted table path, its keys.
# `[ontology]` is on by default in the product, so a control arm turns it off explicitly: the pair differs by it alone.
ONTO_OFF = {"ontology": {"judge": False, "recall_boost": 0.0}}
ONTO_ON = {"ontology": {"judge": True, "recall_boost": 0.25}}  # the shipped defaults, named so the arm stays what it was
OVERRIDES = {"A1": ONTO_OFF, "A2": ONTO_OFF, "A1n": {"prefix": {"enabled": False}, **ONTO_OFF}, "A1o": ONTO_ON}
JUDGMENTS = "judgments.json"
UNMEASURABLE = {"A1p": "the daemon builds one persona-set layer whenever two or more [personas] are configured "
                       "(config.rs `context_paths`, `persona_engaging`) and has no switch for a layer per persona; "
                       "the arm is not built as a product change, so it is not measurable (as A8 is not)"}
ASK_WAIT = 0.1  # seconds between looks at a running ask


def arm_config(cfg: dict, arm: str, personas: dict[str, str], persona_dir: Path) -> dict:
    """The scratch daemon's config for a threads arm: the common one (the five persona cards as `[personas.*]`, each
    with a file of its card, so the persona layer holds them) and the arm's own lines (`OVERRIDES`)."""
    t = json.loads(json.dumps(cfg))
    persona_dir.mkdir(parents=True, exist_ok=True)
    for name, card in personas.items():
        f = persona_dir / f"{name}.md"
        f.write_text(card + "\n")
        t.setdefault("personas", {})[name] = {"files": [str(f)], "card": card}
    for table, keys in OVERRIDES[arm].items():
        t.setdefault(table, {}).update(keys)
    return t


def merge(base: dict, over: dict) -> dict:
    """`over` laid over `base`, table by table (the maintainer's `--config-overlay`: Jev's endpoint, for one)."""
    out = dict(base)
    for k, v in over.items():
        out[k] = merge(out[k], v) if isinstance(v, dict) and isinstance(out.get(k), dict) else v
    return out


def judgment_row(row: dict) -> dict:
    """One `judge.call` ledger row, kept as the few fields the decisions column reads (the state and any text stay in
    the daemon's ledger): the pack, how it ended, what Jev billed, and the tokens it read and wrote; and, when the row
    has them, each answer's question, band and whether it chose something (`answers`), the turn and session it
    judged (`turn`, `session`) and its time (`at_ms`)."""
    d = row.get("data") or row
    outcome = d.get("outcome")
    usage = d.get("usage") or {}
    out = {"pack": d.get("pack"), "outcome": outcome.get("outcome") if isinstance(outcome, dict) else outcome,
           "cost_micros": d.get("cost_micros") or 0, "input_tokens": usage.get("input_tokens") or 0,
           "output_tokens": usage.get("output_tokens") or 0}
    answers = [x for x in map(th_decisions.answer_row, d.get("answers") or []) if x]
    ctx = d.get("context") or {}
    extra = {"answers": answers, "turn": ctx.get("turn"), "session": ctx.get("session"),
             "at_ms": row.get("at_unix_ms") or ctx.get("event_at_ms")}
    return {**out, **{k: v for k, v in extra.items() if v}}


def tag_of(result: dict) -> dict | None:
    """The reply's thread tag, when the place had threads to choose between."""
    t = result.get("thread")
    return t if isinstance(t, dict) else None


def record(m, tag: dict | None, slug_stream: dict[str, str], asked: bool, addressed: str | None,
           routed: dict | None, engaged: dict | None, picks: list[dict]) -> dict:
    """One message's routing record: `th_score.py`'s three fields (`msg`, `stream`, `asked`), then what the daemon
    reported about the placement, the persona and the prefix pick."""
    slug = addressed or (tag or {}).get("slug")
    stream = addressed or (slug_stream.get(slug, f"?{slug}") if slug else HOME)
    rec = {"msg": m.index, "stream": stream, "asked": asked, "thread": slug, "addressed": addressed is not None}
    if tag:
        rec.update({"how": tag.get("how"), "confidence": tag.get("confidence")})
    if routed:
        rec["timed_out"] = routed.get("timed_out")
        rec["jev"] = (routed.get("jev") or {}).get("pick")
    if engaged:
        rec["persona"] = {k: engaged.get(k) for k in ("persona", "name", "how", "p")}
    if picks:
        rec["prefix"] = [{k: p.get(k) for k in ("choice", "fit", "below_act")} for p in picks]
    return rec


def run_ask(cmd: list[str], text: str, timeout: float, on_ask, env: dict) -> tuple[str, str, int | None, list[dict]]:
    """`theseus ask` with `text` on stdin, its stderr read as it comes: when the router asks, `on_ask(ask)` gives the
    key that answers (a `thread answer` goes out beside the turn, which waits for it). Returns stdout, stderr, the
    exit code (None: it ran past `timeout` and its group was killed) and the questions seen."""
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         start_new_session=True, env=env)
    asks: queue.Queue = queue.Queue()
    out_parts: list[str] = []
    err_lines: list[str] = []

    def read_err() -> None:
        question = options = ""
        for line in p.stderr:
            err_lines.append(line)
            if line.startswith("❓"):
                question, options = line.rstrip("\n"), ""
            elif line.strip().startswith("["):
                options = line.strip()
            elif owner_mod.ANSWER_LINE.search(line):
                asks.put(owner_mod.parse_ask(question, options, line))

    readers = [threading.Thread(target=lambda: out_parts.append(p.stdout.read()), daemon=True),
               threading.Thread(target=read_err, daemon=True)]
    for r in readers:
        r.start()
    p.stdin.write(text)
    p.stdin.close()
    seen: list[dict] = []
    deadline = time.monotonic() + timeout
    timed_out = False
    while p.poll() is None:
        try:
            ask = asks.get(timeout=ASK_WAIT)
        except queue.Empty:
            ask = None
        if ask is not None:
            seen.append(ask)
            on_ask(ask)
        if time.monotonic() > deadline:
            timed_out = True
            try:
                import os
                os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()
    for r in readers:
        r.join(10)
    p.stdout.close()
    p.stderr.close()
    err = "".join(err_lines) + ("\ntimed out" if timed_out else "")
    return "".join(out_parts), err, None if timed_out else p.returncode, seen


class ThreadsTheseus(rd.Theseus):
    """The recall bench's scratch daemon for a threads arm: its config, a home session, and one ask per message."""

    def __init__(self, a, run, arm: str, prog, overlay: dict | None = None):
        super().__init__(a, run)
        self.arm, self.tprog = arm, prog
        cfg = arm_config(tomllib.loads(self.cfg.read_text()), arm, prog.personas, self.dir / "personas")
        merged = merge(cfg, overlay or {})
        self.cfg.write_text(rd.toml_dumps(merged))
        run.meta["ontology"] = merged.get("ontology")  # what the arm ran with, kept beside its numbers
        self.owner = owner_mod.Owner()
        self.slug_stream: dict[str, str] = {}  # a thread's slug -> the stream the owner means by it
        self.last: dict[str, int] = {}  # a ledger kind -> the newest position read

    def new_rows(self, kind: str) -> list[dict]:
        """The ledger's rows of `kind` since the last look, oldest first, as their `data`."""
        rows = [x for x in self.ledger(kind) if (x.get("position") or 0) > self.last.get(kind, -1)]
        rows.sort(key=lambda x: x.get("position") or 0)
        if rows:
            self.last[kind] = rows[-1]["position"]
        return [x.get("data") or {} for x in rows]

    def make_thread(self, stream) -> None:
        """The owner makes a stream's thread by hand: its slug is the stream's id, its title and goal the stream's."""
        r = self.cli("--json", "thread", "new", stream.id, "--title", stream.title, "--goal", stream.desc)
        if r.returncode != 0:
            raise RuntimeError(f"thread new {stream.id} failed: {(r.stderr or r.stdout).strip()[-300:]}")
        self.slug_stream[stream.id] = stream.id
        self.owner.title[stream.id] = stream.title
        self.owner.slug[stream.id] = stream.id

    def drive(self) -> str:
        prog, run, a = self.tprog, self.run, self.a
        streams = {s.id: s for s in prog.streams}
        self.start()
        home = self.open_session("threads home")
        run.meta["sessions"] = [home]
        run.meta["reopen_not_driven"] = sum(1 for m in prog.messages if m.reopen)
        for k in ("thread.routed", "persona.engaged", "prefix.picked", "thread.asked"):
            self.new_rows(k)  # nothing before the first message counts
        records: list[dict] = []
        transcript: list[str] = []
        by_turn = {t.index: t for t in run.prog.turns}
        by_index = {m.index: m for m in prog.messages}
        out_f = (run.out / ROUTING).open("w")
        try:
            for m in prog.messages:
                t = by_turn[m.index]
                pg.apply_before(t, run.workspace)
                first = self.owner.first_of_stream(m.stream)
                addressed = self.arm == "A2" and m.stream != HOME
                if addressed and m.stream not in self.slug_stream:
                    self.make_thread(streams[m.stream])
                target = ["--thread", m.stream] if addressed else ["-s", home]
                answered: list[dict] = []

                def on_ask(ask: dict, stream=m.stream) -> None:
                    key = self.owner.answer(stream, ask)
                    r = self.cli("thread", "answer", ask["id"], key)
                    answered.append({**ask, "choice": key, "sent": r.returncode == 0})

                t0 = time.monotonic()
                out, err, code, _ = run_ask([str(self.theseus), "--socket", str(self.sock), "--json", "ask", *target, "-"],
                                            t.text, a.turn_timeout, on_ask, self.env)
                ms = int((time.monotonic() - t0) * 1000)
                if code is None:
                    self.cli("stop", home, timeout=60)
                (run.raw / f"t{t.index:04d}.json").write_text(out)
                try:
                    v = json.loads(out) if out.strip() else {}
                except json.JSONDecodeError:
                    v = {}
                tag = tag_of(v)
                slug = (tag or {}).get("slug")
                # An accepted offer, or a question answered `new`, made the thread: it is the stream's. A thread the
                # owner makes by hand when the product offered nothing is the stream's too.
                if slug and slug not in self.slug_stream and any(x["choice"] == "new" for x in answered):
                    self.slug_stream[slug] = m.stream
                    self.owner.title[m.stream] = tag.get("title", slug)
                    self.owner.slug[m.stream] = slug
                if not addressed and self.owner.needs_thread(m.stream, first, slug) and not answered:
                    self.make_thread(streams[m.stream])
                    self.owner.started_by_hand += 1
                self.owner.sent(m.stream)
                routed, engaged = (self.new_rows("thread.routed") or [None])[-1], (self.new_rows("persona.engaged") or [None])[-1]
                rec = record(m, tag, self.slug_stream, bool(answered), m.stream if addressed else None, routed,
                             engaged, self.new_rows("prefix.picked"))
                records.append(rec)
                out_f.write(json.dumps(rec, sort_keys=True) + "\n")
                out_f.flush()
                run.turn({
                    "index": t.index, "session": t.session, "session_id": home, "role": t.role,
                    "reply": v.get("output", ""), "stop": v.get("stop_reason"), "exit": code,
                    "error": err.strip()[-400:] if code != 0 else None, "tokens": rd._tokens(v.get("usage")),
                    "cost_usd": v.get("cost_usd"), "latency_ms": ms, "compacted": False,
                    "tool_calls": v.get("tool_calls"), "ran_in": rec["thread"] or HOME,
                    "turn_id": v.get("turn_id"), "ended_ms": int(time.time() * 1000),
                })
                run.save()
            sessions = self.thread_sessions()
            for name, sid in {HOME: home, **sessions}.items():
                text, whole = rd.history_pages(lambda *x: self.cli(*x, timeout=120), sid)
                (run.raw / f"history-{name}.json").write_text(text)
                transcript.extend(rd.strings(whole)) if whole is not None else transcript.append(text)
            run.meta["thread_sessions"] = sessions
            # Jev's judgments over the whole run (the sink writes them between turns, so only now are they all in).
            (run.out / JUDGMENTS).write_text(json.dumps([judgment_row(x) for x in self.ledger("judge.call")]))
        finally:
            out_f.close()
            run.meta["killed"] = self.stop()
            run.meta["left_running"] = [f"{p} {pid}" for pid, p in rd.processes_naming(run.out)]
            run.meta["owner"] = {"over_split": self.owner.over_split, "started_by_hand": self.owner.started_by_hand,
                                 "answers": self.owner.answers}
            run.save()
        return "\n".join(transcript)

    def thread_sessions(self) -> dict[str, str]:
        """Each thread's session, by slug, from `thread list` (kept whole in raw/threads.json)."""
        r = self.cli("--json", "thread", "list")
        (self.run.raw / "threads.json").write_text(r.stdout or r.stderr)
        try:
            rows = json.loads(r.stdout).get("threads", [])
        except json.JSONDecodeError:
            return {}
        return {x["thread"]["slug"]: x["thread"]["session_id"] for x in rows if x.get("thread", {}).get("session_id")}

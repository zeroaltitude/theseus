#!/usr/bin/env python3
"""openclaw-export.py: OpenClaw's own session stores, as episodes for Theseus's import.

Reads `<state>/agents/<agent>/agent/openclaw-agent.sqlite` (read only) and writes
episode lines (format 1: crates/theseus-core/src/import/episode.rs) to stdout, one
episode per OpenClaw session, for `theseus import openclaw -` or, run by
`theseus import sync`, straight into the daemon. Python's standard library only;
Node (OpenClaw's own runtime) decodes a zstd event when Python has no zstd of its own.

What it reads (the shapes are OpenClaw's, src/state/openclaw-agent-schema.sql, pinned in
scripts/tests/openclaw_fixture.py):
  session_windows               one row a session: key, channel, chat type, times, owner
  transcript_events             session_id, seq, event_json or event_zstd, created_at
  transcript_rewrite_watermarks session_id, generation, updated_at (a rewrite)
  session_transcript_archives   a deleted session's transcript, a JSONL blob

What it writes: one episode a session, its id stable for the session's life
(`ep_` + sha256 of `openclaw-sync`, the agent and the session id, NUL-joined). Each
user or assistant text message is a message whose `idx` is its `seq`, so a session that
grew is the same episode with more messages after the stored ones, which the daemon
appends (import/append.rs). Custom, session, thinking-level and every other event is
skipped and counted, as are tool results, thinking and tool calls inside a message.
There is no triage, no segmentation and no summary.

Integrity and author: the owner's turns are `operator` (the owner's ids come from
--owner-id, which the daemon reads from the owner's config, never this repository); the
agent's are `agent:<agent>`; another person's channel message is `outside` and
`person:<name>`. A message from a webhook or mail hook is `outside`. Scheduled prompts
(cron, heartbeat) are the owner's own configuration: `operator`, author `schedule`.

The place comes from the session key and channel: `dm`, `slack-channel`,
`discord-channel`, `cron`, `heartbeat` or `cli`. Sensitivity is `personal` for a DM and
`company-confidential` otherwise. No partner label is ever set.

The cursor (--cursor-in / --cursor-out, JSON) is per agent: the newest
`transcript_events` rowid read, the newest archive rowid read, and the newest rewrite
watermark time. A run reads only rows past it, and a night with nothing new is three
one-row reads an agent. `floor_ms` is the first run's start: a message made before it is
never exported (the 2026-10-06 import holds that history), for any session, always, so a
session's message list stays a prefix of its own later one.

Every statement is short, on its own read-only connection state (`mode=ro`,
`query_only`), with no transaction held across sessions.
"""

import argparse
import base64
import datetime
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys

SOURCE = "openclaw-sync"
FORMAT = 1
CHUNK = 256  # rows a fetch, and zstd blobs a decode call


# ---------------------------------------------------------------- zstd -----


def _python_zstd():
    """A decompress(bytes) -> bytes from Python's own, or None."""
    try:  # 3.14+
        from compression import zstd  # type: ignore

        return zstd.decompress
    except Exception:
        pass
    try:
        import zstandard  # type: ignore

        return lambda b: zstandard.ZstdDecompressor().decompressobj().decompress(b)
    except Exception:
        return None


NODE_DECODER = (
    "const z=require('zlib');let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{"
    "const a=JSON.parse(s);process.stdout.write(JSON.stringify(a.map(b=>{try{"
    "return z.zstdDecompressSync(Buffer.from(b,'base64')).toString('utf8')}"
    "catch(e){return null}})))});"
)


class Undecodable(Exception):
    """An event that cannot be read at all: the session is not exported this run."""


class Zstd:
    """Decodes zstd blobs: Python's codec, the `zstd` program, then Node's zlib."""

    def __init__(self):
        self.py = _python_zstd()
        self.cli = shutil.which("zstd")
        self.node = shutil.which("node")
        self.failed = 0

    def available(self):
        return bool(self.py or self.cli or self.node)

    def decode(self, blobs):
        """Each blob's text (UTF-8), or None where it cannot be decoded."""
        out = [None] * len(blobs)
        if self.py:
            for i, b in enumerate(blobs):
                try:
                    out[i] = self.py(bytes(b)).decode("utf-8")
                except Exception:
                    pass
        elif self.cli:
            for i, b in enumerate(blobs):
                try:
                    r = subprocess.run(
                        [self.cli, "-d", "-c", "-q"], input=bytes(b), capture_output=True
                    )
                    if r.returncode == 0:
                        out[i] = r.stdout.decode("utf-8")
                except Exception:
                    pass
        elif self.node:
            for lo in range(0, len(blobs), CHUNK):
                part = [base64.b64encode(bytes(b)).decode() for b in blobs[lo : lo + CHUNK]]
                try:
                    r = subprocess.run(
                        [self.node, "-e", NODE_DECODER],
                        input=json.dumps(part).encode(),
                        capture_output=True,
                    )
                    got = json.loads(r.stdout) if r.returncode == 0 else []
                except Exception:
                    got = []
                for j, t in enumerate(got):
                    out[lo + j] = t
        self.failed += sum(1 for t in out if t is None)
        return out


# ------------------------------------------------------------ the store -----


def agents_of(state, only):
    """[(agent name, sqlite path)] for each agent store under the state dir."""
    base = os.path.join(state, "agents")
    found = []
    try:
        names = sorted(os.listdir(base))
    except OSError:
        return found
    for name in names:
        path = os.path.join(base, name, "agent", "openclaw-agent.sqlite")
        if os.path.isfile(path) and (not only or name in only):
            found.append((name, path))
    return found


def open_ro(path):
    uri = "file:" + path.replace("?", "%3f").replace("#", "%23") + "?mode=ro"
    db = sqlite3.connect(uri, uri=True, timeout=5.0, isolation_level=None)
    db.execute("PRAGMA query_only = ON")
    return db


def one(db, sql, args=()):
    """One short statement, its first row's first column (or None)."""
    row = db.execute(sql, args).fetchone()
    return row[0] if row else None


def has_table(db, name):
    return (
        one(db, "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,))
        is not None
    )


# ---------------------------------------------------------------- events -----

META_BLOCK = re.compile(
    r"^(?:Conversation info|Sender|Forwarded message context|Location|Structured object)"
    r" \(untrusted metadata\):[ \t]*\n```json\n(.*?)\n```[ \t]*\n?",
    re.M | re.S,
)


def block_text(content):
    """The text blocks of a message's content, and how many other blocks it held."""
    if isinstance(content, str):
        return content, {}
    parts, other = [], {}
    for b in content if isinstance(content, list) else []:
        if isinstance(b, dict) and b.get("type") == "text" and isinstance(b.get("text"), str):
            parts.append(b["text"])
        elif isinstance(b, dict):
            k = "block:" + str(b.get("type", "unknown"))
            other[k] = other.get(k, 0) + 1
    return "\n".join(parts), other


def sender_of(text):
    """The inbound envelope's sender, its metadata blocks taken out of the text."""
    sender = {}
    for m in META_BLOCK.finditer(text):
        head = text[m.start() : m.start() + 12]
        if head.startswith("Sender"):
            try:
                v = json.loads(m.group(1))
                if isinstance(v, dict):
                    sender = v
            except ValueError:
                pass
    return sender, META_BLOCK.sub("", text).strip()


def iso(ms):
    d = datetime.datetime.fromtimestamp(ms / 1000.0, datetime.timezone.utc)
    return d.strftime("%Y-%m-%dT%H:%M:%S.") + "%03dZ" % (ms % 1000)


def place_of(key, channel, chat_type, conv, display):
    """(kind, name, id, is_scheduled) from the session key and channel."""
    rest = key.split(":", 2)[2] if key.count(":") >= 2 and key.startswith("agent:") else key
    first = rest.split(":", 1)[0]
    if first == "cron" or channel == "cron":
        return "cron", display, None, True
    if "heartbeat" in rest.split(":") or channel == "heartbeat":
        return "heartbeat", display, None, True
    if channel and chat_type == "direct":
        return "dm", display, conv, False
    if channel == "slack" and chat_type in ("group", "channel"):
        return "slack-channel", display, conv, False
    if channel == "discord" and chat_type in ("group", "channel"):
        cid = conv if conv and str(conv).isdigit() else None
        return "discord-channel", display, cid, False
    return "cli", display, None, False


def message_of(event, ctx, counts):
    """One transcript event as a message dict (no idx/time), or None, counted."""
    t = event.get("type") if isinstance(event, dict) else None
    if t != "message":
        counts[str(t or "unknown")] = counts.get(str(t or "unknown"), 0) + 1
        return None
    m = event.get("message")
    role = m.get("role") if isinstance(m, dict) else None
    if role not in ("user", "assistant"):
        counts["role:" + str(role)] = counts.get("role:" + str(role), 0) + 1
        return None
    text, other = block_text(m.get("content"))
    for k, n in other.items():
        counts[k] = counts.get(k, 0) + n
    if role == "assistant":
        text = text.strip()
        if not text:
            counts["empty"] = counts.get("empty", 0) + 1
            return None
        return {"author": "agent:" + ctx["agent"], "integrity": "agent", "text": text}
    sender, text = sender_of(text)
    if not text:
        counts["empty"] = counts.get("empty", 0) + 1
        return None
    sid = str(sender.get("id")) if sender.get("id") is not None else None
    name = sender.get("name") or sender.get("label") or sender.get("username") or sid
    owner = sid is not None and sid in ctx["owners"]
    if ctx["external"]:
        return {"author": "outside", "integrity": "outside", "text": text}
    if ctx["scheduled"]:
        return {"author": "schedule", "integrity": "operator", "text": text}
    if owner:
        return {"author": "operator", "integrity": "operator", "text": text}
    if not sender and ctx["kind"] in ("cli", "dm") and ctx["direct_is_owner"]:
        # The CLI and the main session carry no envelope: the owner at their keyboard.
        return {"author": "operator", "integrity": "operator", "text": text}
    if name:
        return {"author": "person:" + str(name).strip(), "integrity": "outside", "text": text}
    return {"author": "outside", "integrity": "outside", "text": text}


# -------------------------------------------------------------- episodes -----


def episode_id(agent, session_id):
    h = hashlib.sha256("\0".join([SOURCE, agent, session_id]).encode("utf-8"))
    return "ep_" + h.hexdigest()


def canonical_hash(v):
    v = dict(v)
    v.pop("hash", None)
    s = json.dumps(v, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def episode_of(agent, session_id, win, messages, tag):
    kind, name, pid, scheduled = place_of(
        win["key"], win["channel"], win["chat_type"], win["conv"], win["display"]
    )
    out = {
        "format": FORMAT,
        "import_tag": tag,
        "episode_id": episode_id(agent, session_id),
        "source": SOURCE,
        "agent": agent,
        "place": {"kind": kind, "name": name, "id": pid},
        "as_of": {"start": messages[0]["time"], "end": messages[-1]["time"]},
        "labels": {
            "sensitivity": "personal" if kind == "dm" else "company-confidential",
            "partner": None,
            "topic": [],
            "book_hint": None,
            "credential_redacted": False,
        },
        "summary": None,
        "messages": messages,
    }
    out["hash"] = canonical_hash(out)
    return out


def finish_messages(rows, agent, session_id, ctx, floor_ms, zstd, counts):
    """Messages from (seq, json text or None, created_at) rows, in seq order."""
    messages = []
    for seq, text, created in rows:
        if created < floor_ms:
            counts["before_floor"] = counts.get("before_floor", 0) + 1
            continue
        if text is None:
            # A hole in the middle would break the prefix every later run depends on.
            raise Undecodable(
                "event %s of session %s is zstd and cannot be decoded (Python has no zstd, and neither"
                " the zstd program nor node is on the path)" % (seq, session_id)
            )
        try:
            event = json.loads(text)
        except ValueError:
            counts["unreadable"] = counts.get("unreadable", 0) + 1
            continue
        m = message_of(event, ctx, counts)
        if m is None:
            continue
        m.update(
            idx=int(seq),
            time=iso(int(created)),
            unit="%s/%s#%d" % (agent, session_id, seq),
            sha256=hashlib.sha256(m["text"].encode("utf-8")).hexdigest(),
        )
        messages.append(m)
    return messages


def window_of(db, session_id):
    r = db.execute(
        "SELECT session_key, channel, chat_type, primary_conversation_id, display_name,"
        " hook_external_content_source FROM session_windows WHERE session_id=?",
        (session_id,),
    ).fetchone()
    if not r:
        return None
    return {
        "key": r[0],
        "channel": r[1],
        "chat_type": r[2],
        "conv": r[3],
        "display": r[4],
        "external": r[5] is not None,
    }


def live_rows(db, session_id, zstd):
    """A live session's (seq, event json, created_at) rows in order, zstd decoded."""
    cur = db.execute(
        "SELECT seq, event_json, event_zstd, created_at FROM transcript_events"
        " WHERE session_id=? ORDER BY seq",
        (session_id,),
    )
    rows = []
    while True:
        part = cur.fetchmany(CHUNK)
        if not part:
            break
        packed = [i for i, r in enumerate(part) if r[1] is None and r[2] is not None]
        decoded = zstd.decode([part[i][2] for i in packed]) if packed else []
        by = dict(zip(packed, decoded))
        for i, r in enumerate(part):
            rows.append((r[0], r[1] if r[1] is not None else by.get(i), r[3]))
    return rows


def archive_rows(blob, encoding, created_at, zstd):
    """A deleted session's rows from its archive: one event a line, seq the line's index."""
    if encoding == "zstd":
        text = zstd.decode([blob])[0]
    else:
        try:
            text = bytes(blob).decode("utf-8")
        except UnicodeDecodeError:
            text = None
    if text is None:
        return None
    rows = []
    for n, line in enumerate(l for l in text.split("\n") if l.strip()):
        stamp = created_at
        try:
            e = json.loads(line)
            ts = e.get("timestamp") if isinstance(e, dict) else None
            if isinstance(ts, str):
                stamp = int(
                    datetime.datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()
                    * 1000
                )
            elif isinstance(ts, (int, float)):
                stamp = int(ts)
        except Exception:
            pass
        rows.append((n, line, stamp))
    return rows


def run(args):
    now_ms = args.now_ms if args.now_ms is not None else int(datetime.datetime.now().timestamp() * 1000)
    tag = args.tag or datetime.datetime.fromtimestamp(now_ms / 1000, datetime.timezone.utc).strftime(
        "openclaw-sync-%Y-%m"
    )
    cursor = {"version": 1, "floor_ms": None, "agents": {}}
    if args.cursor_in and os.path.exists(args.cursor_in):
        with open(args.cursor_in) as f:
            cursor = json.load(f)
    if cursor.get("floor_ms") is None:
        if args.since_ms is None:
            sys.exit(
                "openclaw-export: no cursor and no --since: the first run needs its start "
                "(theseus import sync reads it from the 2026-10-06 import's newest session)"
            )
        cursor["floor_ms"] = int(args.since_ms)
    elif args.since_ms is not None:
        sys.exit(
            "openclaw-export: a cursor exists (floor %s): --since only sets the first run's start"
            % iso(cursor["floor_ms"])
        )
    floor = int(cursor["floor_ms"])
    owners = set(args.owner_id or [])
    zstd = Zstd()
    report = {"episodes": 0, "messages": 0, "agents": {}, "dry_run": args.dry_run, "tag": tag}
    out = sys.stdout if args.out == "-" else open(args.out, "w")
    new_cursor = {"version": 1, "floor_ms": floor, "agents": {}}
    for agent, path in agents_of(args.state, set(args.agents or [])):
        a = {"sessions": 0, "messages": 0, "archives": 0, "skipped": {}, "errors": []}
        report["agents"][agent] = a
        prev = cursor["agents"].get(agent, {})
        try:
            db = open_ro(path)
            ev_top = one(db, "SELECT max(rowid) FROM transcript_events") or 0
            ar_top = (
                one(db, "SELECT max(rowid) FROM session_transcript_archives")
                if has_table(db, "session_transcript_archives")
                else 0
            ) or 0
            wm_top = (
                one(db, "SELECT max(updated_at) FROM transcript_rewrite_watermarks")
                if has_table(db, "transcript_rewrite_watermarks")
                else 0
            ) or 0
            ev0, ar0, wm0 = prev.get("events_rowid", 0), prev.get("archives_rowid", 0), prev.get("watermark_ms", 0)
            new_cursor["agents"][agent] = {
                "events_rowid": ev_top,
                "archives_rowid": ar_top,
                "watermark_ms": max(wm_top, wm0),
            }
            if (ev_top, ar_top, wm_top) == (ev0, ar0, wm0):
                continue  # a night with nothing new: the three reads above
            changed = set()
            if ev_top > ev0:
                for (s,) in db.execute(
                    "SELECT DISTINCT session_id FROM transcript_events WHERE rowid > ? AND rowid <= ?",
                    (ev0, ev_top),
                ):
                    changed.add(s)
            if wm_top > wm0:
                for (s,) in db.execute(
                    "SELECT session_id FROM transcript_rewrite_watermarks WHERE updated_at > ?",
                    (wm0,),
                ):
                    changed.add(s)
            for session_id in sorted(changed):
                win = window_of(db, session_id)
                if win is None:
                    continue  # deleted since: its archive carries it
                emit(db, agent, session_id, win, None, owners, floor, zstd, a, report, out, tag, args)
            if ar_top > ar0:
                done = set()
                archives = db.execute(
                    "SELECT rowid, session_id, encoding, archive_blob, created_at FROM"
                    " session_transcript_archives WHERE rowid > ? AND rowid <= ?"
                    " AND reason = 'deleted' ORDER BY rowid",
                    (ar0, ar_top),
                ).fetchall()
                for rowid, session_id, encoding, blob, created in archives:
                    if session_id in done or window_of(db, session_id) is not None:
                        continue  # a live session of that id has its own rows
                    done.add(session_id)
                    key = one(
                        db,
                        "SELECT session_key FROM session_transcript_archives WHERE rowid=?",
                        (rowid,),
                    )
                    win = {"key": key, "channel": None, "chat_type": None, "conv": None,
                           "display": None, "external": False}
                    rows = archive_rows(blob, encoding, created, zstd)
                    if rows is None:
                        a["errors"].append("archive of %s could not be decoded" % session_id)
                        continue
                    a["archives"] += 1
                    emit(db, agent, session_id, win, rows, owners, floor, zstd, a, report, out, tag, args)
            db.close()
        except sqlite3.Error as e:
            a["errors"].append("%s: %s" % (type(e).__name__, e))
            # An agent that did not read keeps its old cursor.
            new_cursor["agents"][agent] = prev or {"events_rowid": 0, "archives_rowid": 0, "watermark_ms": 0}
    if args.out != "-":
        out.close()
    report["zstd_failed"] = zstd.failed
    for agent, c in cursor["agents"].items():
        new_cursor["agents"].setdefault(agent, c)
    report["cursor"] = new_cursor
    return report


def emit(db, agent, session_id, win, rows, owners, floor, zstd, a, report, out, tag, args):
    place_kind, _, _, scheduled = place_of(
        win["key"] or "", win["channel"], win["chat_type"], win["conv"], win["display"]
    )
    ctx = {
        "agent": agent,
        "owners": owners,
        "external": win["external"],
        "scheduled": scheduled,
        "kind": place_kind,
        # Without an envelope the session is the owner's own: the main session and the CLI.
        "direct_is_owner": True,
    }
    counts = a["skipped"]
    try:
        if rows is None:
            rows = live_rows(db, session_id, zstd)
        messages = finish_messages(rows, agent, session_id, ctx, floor, zstd, counts)
    except Undecodable as e:
        a["errors"].append(str(e))
        return
    if not messages:
        return
    a["sessions"] += 1
    a["messages"] += len(messages)
    report["episodes"] += 1
    report["messages"] += len(messages)
    if args.dry_run:
        return
    ep = episode_of(agent, session_id, win, messages, tag)
    out.write(json.dumps(ep) + "\n")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--from", dest="state", required=True, help="OpenClaw's state directory")
    p.add_argument("--out", default="-", help="episode lines (default stdout)")
    p.add_argument("--report", help="the run's counts and new cursor, as JSON")
    p.add_argument("--cursor-in", help="the last run's cursor (JSON)")
    p.add_argument("--since-ms", type=int, help="the first run's start, unix ms")
    p.add_argument("--agents", type=lambda s: [x for x in s.split(",") if x])
    p.add_argument("--tag", help="import tag (default openclaw-sync-YYYY-MM of the run)")
    p.add_argument("--owner-id", action="append", help="a sender id that is the owner's")
    p.add_argument("--now-ms", type=int, help="the run's clock, for tests")
    p.add_argument("--dry-run", action="store_true", help="count; write no episode")
    args = p.parse_args(argv)
    report = run(args)
    text = json.dumps(report)
    if args.report:
        with open(args.report, "w") as f:
            f.write(text)
    else:
        sys.stderr.write(text + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Small OpenClaw stores for the exporter's tests: invented text, the columns the
exporter reads, as OpenClaw's own schema (src/state/openclaw-agent-schema.sql at the
2026.10 release) names them. Pinned here so a change in the exporter's reads shows."""

import datetime
import json
import os
import sqlite3
import subprocess
import shutil

SCHEMA = """
CREATE TABLE session_windows (
  session_id TEXT NOT NULL PRIMARY KEY, session_key TEXT NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  transcript_updated_at INTEGER DEFAULT NULL,
  hook_external_content_source TEXT, chat_type TEXT, channel TEXT,
  primary_conversation_id TEXT, display_name TEXT) STRICT;
CREATE TABLE transcript_events (
  session_id TEXT NOT NULL, seq INTEGER NOT NULL, event_json TEXT,
  created_at INTEGER NOT NULL, event_zstd BLOB, event_utf8_bytes INTEGER,
  navigation_json TEXT, PRIMARY KEY (session_id, seq)) STRICT;
CREATE TABLE transcript_event_identities (
  session_id TEXT NOT NULL, event_id TEXT NOT NULL, seq INTEGER NOT NULL,
  PRIMARY KEY (session_id, event_id)) STRICT;
CREATE TABLE transcript_rewrite_watermarks (
  session_id TEXT NOT NULL PRIMARY KEY, generation TEXT NOT NULL,
  updated_at INTEGER NOT NULL) STRICT;
CREATE TABLE session_transcript_archives (
  session_id TEXT NOT NULL, generation TEXT NOT NULL, session_key TEXT NOT NULL,
  reason TEXT NOT NULL, encoding TEXT NOT NULL, archive_blob BLOB NOT NULL,
  archive_sha256 TEXT NOT NULL, archive_name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL, PRIMARY KEY (session_id, generation)) STRICT;
"""

T0 = 1_790_000_000_000  # a time in 2026-09, ms


def isoz(ms):
    d = datetime.datetime.fromtimestamp(ms / 1000.0, datetime.timezone.utc)
    return d.strftime("%Y-%m-%dT%H:%M:%SZ")


def token():
    """A key-shaped value, built from parts."""
    return "ghp" + "_" + "Zy7aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0"


def store(state, agent="main"):
    d = os.path.join(state, "agents", agent, "agent")
    os.makedirs(d, exist_ok=True)
    db = sqlite3.connect(os.path.join(d, "openclaw-agent.sqlite"), isolation_level=None)
    for stmt in SCHEMA.split(";"):
        if stmt.strip():
            db.execute(stmt)
    return db


def zstd_blob(text):
    """A real zstd frame of `text`, made by Node's zlib (OpenClaw's own codec)."""
    node = shutil.which("node")
    if not node:
        return None
    code = (
        "const z=require('zlib');let s='';process.stdin.on('data',d=>s+=d).on('end',()=>"
        "process.stdout.write(z.zstdCompressSync(Buffer.from(s,'utf8'))))"
    )
    r = subprocess.run([node, "-e", code], input=text.encode(), capture_output=True)
    return r.stdout if r.returncode == 0 and r.stdout else None


def window(db, sid, key, t, chat_type=None, channel=None, conv=None, display=None, hook=None):
    db.execute(
        "INSERT INTO session_windows (session_id, session_key, created_at, updated_at,"
        " hook_external_content_source, chat_type, channel, primary_conversation_id,"
        " display_name) VALUES (?,?,?,?,?,?,?,?,?)",
        (sid, key, t, t, hook, chat_type, channel, conv, display),
    )


def msg(role, text, ts=None):
    m = {"role": role, "content": [{"type": "text", "text": text}]}
    return {"type": "message", "id": "e", "parentId": None, "timestamp": ts, "message": m}


def event(db, sid, seq, ev, created, zstd=False):
    text = json.dumps(ev)
    if zstd:
        blob = zstd_blob(text)
        db.execute(
            "INSERT INTO transcript_events (session_id, seq, event_json, created_at,"
            " event_zstd, event_utf8_bytes, navigation_json) VALUES (?,?,?,?,?,?,?)",
            (sid, seq, None, created, blob, len(text.encode()), "{}"),
        )
    else:
        db.execute(
            "INSERT INTO transcript_events (session_id, seq, event_json, created_at)"
            " VALUES (?,?,?,?)",
            (sid, seq, text, created),
        )


def envelope(sender_id, name, body):
    """A channel message as OpenClaw's inbound envelope writes it."""
    meta = json.dumps({"id": sender_id, "name": name, "label": name})
    return (
        "Conversation info (untrusted metadata):\n```json\n"
        + json.dumps({"conversation_label": "reef-team"})
        + "\n```\n\nSender (untrusted metadata):\n```json\n"
        + meta
        + "\n```\n\n"
        + body
    )


def build(state, with_zstd=True):
    """The fixture state: a DM, a channel with another person, a cron run, a
    session with an unknown event type, a rewritten-later session, a zstd event, a
    planted secret, and a deleted session in the archive."""
    db = store(state)
    t = T0
    # A DM (the owner's id is U-OWNER), a custom event and a thinking-level event.
    window(db, "s-dm", "agent:main:slack:direct:U-OWNER", t, "direct", "slack", "D01", "wren")
    event(db, "s-dm", 0, {"type": "session", "version": 3, "id": "s-dm"}, t)
    event(db, "s-dm", 1, {"type": "thinking_level_change", "thinkingLevel": "low"}, t + 1)
    event(db, "s-dm", 2, msg("user", envelope("U-OWNER", "Wren", "Where does the tide log live?")), t + 2)
    event(db, "s-dm", 3, msg("assistant", "The tide log is in the boathouse ledger. The deploy key is " + token()), t + 3)
    event(db, "s-dm", 4, {"type": "custom", "customType": "x", "data": {}}, t + 4)
    # A channel with another person.
    window(db, "s-ch", "agent:main:slack:channel:C01", t, "channel", "slack", "C01", "reef-team")
    event(db, "s-ch", 0, msg("user", envelope("U-OTHER", "Quill", "Can anyone move the quay survey?")), t + 10)
    event(db, "s-ch", 1, msg("user", envelope("U-OWNER", "Wren", "I will take the quay survey.")), t + 11)
    event(db, "s-ch", 2, msg("assistant", "Noted: Wren takes the quay survey."), t + 12)
    # A cron run, with an event of a type the exporter does not know.
    window(db, "s-cron", "agent:main:cron:nightly", t, None, "cron", None, "nightly-tide")
    event(db, "s-cron", 0, msg("user", "Check the tide tables and report."), t + 20)
    event(db, "s-cron", 1, {"type": "brand_new_thing", "x": 1}, t + 21)
    event(db, "s-cron", 2, msg("assistant", "Tide tables checked: low at dawn."), t + 22)
    # A session whose message is compressed.
    window(db, "s-zstd", "agent:main:main", t, None, None, None, None)
    event(db, "s-zstd", 0, msg("user", "A compressed question about the reef survey."), t + 30, zstd=with_zstd)
    event(db, "s-zstd", 1, msg("assistant", "A compressed answer about the reef survey."), t + 31, zstd=with_zstd)
    # A session deleted since: its transcript is in the archive.
    lines = "\n".join(
        json.dumps(e)
        for e in [
            {"type": "session", "id": "s-gone"},
            msg("user", "A question in a session since deleted.", isoz(T0 + 41_000)),
            msg("assistant", "An answer in a session since deleted.", isoz(T0 + 42_000)),
        ]
    )
    db.execute(
        "INSERT INTO session_transcript_archives (session_id, generation, session_key, reason,"
        " encoding, archive_blob, archive_sha256, archive_name, created_at)"
        " VALUES (?,?,?,?,?,?,?,?,?)",
        ("s-gone", "g1", "agent:main:main", "deleted", "identity", lines.encode(), "0" * 64, "gone.jsonl", t + 40),
    )
    return db


def grow(state):
    """One more message in the DM, and a new session: the next night's changes."""
    path = os.path.join(state, "agents", "main", "agent", "openclaw-agent.sqlite")
    db = sqlite3.connect(path, isolation_level=None)
    event(db, "s-dm", 5, msg("user", envelope("U-OWNER", "Wren", "And the quay survey's key?")), T0 + 99)
    window(db, "s-new", "agent:main:main", T0 + 100)
    event(db, "s-new", 0, msg("user", "A fresh question about the kelp count."), T0 + 100)
    event(db, "s-new", 1, msg("assistant", "A fresh answer about the kelp count."), T0 + 101)
    db.close()


if __name__ == "__main__":
    import sys

    cmd, arg = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else None)
    if cmd == "build":
        build(arg).close()
    elif cmd == "grow":
        grow(arg)
    elif cmd == "since":
        print(isoz(T0 - 1000))

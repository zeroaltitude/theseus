#!/usr/bin/env python3
"""notes-export.py: an agent's own operating notes, as episodes for Theseus's import.

Walks a notes directory and writes episode lines (format 1:
crates/theseus-core/src/import/episode.rs) to stdout, one episode per note file, for
`theseus import openclaw -`. Python's standard library only, reads only, writes nothing
but stdout and a summary on stderr.

This is the read path for `b4-theseus-holds-its-own-notes` (row B4): everything an agent
needs in order to work ON Theseus -- how we build, what the gates are, how the lanes and
the cloud routine work, the standing decisions -- held in Theseus's own memory rather
than only in the workspace it was written in.

ONE NOTE IS ONE EPISODE, with one message holding the whole file, verbatim:

  source       always `agent-notes`, which the daemon gates (see "The switch" below).
  episode_id   `ep_` + sha256 of the source, the note's path and its TEXT, NUL-joined.
               The text is in the id on purpose: an EDITED note is a NEW episode, and
               the one it replaces stays in the store as history. The record only grows,
               and nothing has to decide whether an edit is an append.
  place        `{"kind": "file", "name": <path relative to --root>}`. The name is what
               the durable-versus-run-state rule reads as the note's name, so the
               relative path matters: a note under a `log/` or a dated directory is read
               as a dated log by its path, not only by its own file name.
  as_of        the file's modification time, start and end alike. A note is a document
               and not a conversation: it has no span.
  labels       sensitivity `company-confidential` (operating notes are internal; pass
               --sensitivity to say otherwise), and the note's parent directories as
               topic labels, so `import topics` can make them the ontology's topics.
  messages     exactly one: idx 0, the file's whole text, the file's mtime, the author
               and integrity from --author/--integrity, its unit the relative path and
               its sha256 the text's.

THE SWITCH. `[import.notes]` in the daemon's config is OFF by default, and while it is
off every episode of this source is REFUSED and the refusal names what the
durable-versus-run-state rule read the note as, with its reason. So the way to see what
the rule would do to a notes directory is to export it and import it with the switch
off: the import runs, every note comes back classified, and nothing is written. The rule
is docs/design/agent-notes-rule.md. Do not classify notes here: the rule has one
implementation and it is the daemon's, so this script and the daemon can never disagree.

WHAT IT LEAVES OUT, and counts: a dotfile or a dotted directory, a file whose extension
is not in --ext, one over --max-bytes, one that is not valid UTF-8, an empty one, and one
whose name reads as a secret store (`.env`, `id_rsa`, anything holding `secret`, `token`,
`credential`, `password`, or ending `.pem` or `.key`). The daemon scrubs vault values out
of what it stores in any case; the name check is a second door, not the only one.
"""

import argparse
import datetime
import hashlib
import json
import os
import sys

SOURCE = "agent-notes"
FORMAT = 1

#: Extensions taken by default: prose notes, nothing executable.
DEFAULT_EXT = "md,markdown,txt,rst"

#: A file's longest text, in bytes. A note past this is a dump, not a note.
DEFAULT_MAX_BYTES = 256 * 1024

#: What a name says a file holds, whatever its extension.
SECRET_NAMES = ("secret", "token", "credential", "password", "passwd", "apikey", "api_key")
SECRET_WHOLE = (".env", "id_rsa", "id_ed25519", ".netrc", ".htpasswd")
SECRET_SUFFIX = (".pem", ".key", ".p12", ".pfx", ".jks")


def iso(ms):
    d = datetime.datetime.fromtimestamp(ms / 1000.0, datetime.timezone.utc)
    return d.strftime("%Y-%m-%dT%H:%M:%S.") + "%03dZ" % (ms % 1000)


def canonical_hash(v):
    s = json.dumps(v, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def episode_id(rel, text):
    h = hashlib.sha256("\0".join([SOURCE, rel, text]).encode("utf-8"))
    return "ep_" + h.hexdigest()


def looks_like_a_secret(name):
    low = name.lower()
    if low in SECRET_WHOLE or low.endswith(SECRET_SUFFIX):
        return True
    return any(w in low for w in SECRET_NAMES)


def topics_of(rel):
    """The note's parent directories, as topic labels, outermost first."""
    parts = rel.replace("\\", "/").split("/")[:-1]
    return [p for p in parts if p]


def episode_of(rel, text, mtime_ms, args):
    when = iso(mtime_ms)
    message = {
        "idx": 0,
        "time": when,
        "author": args.author,
        "integrity": args.integrity,
        "text": text,
        "unit": rel,
        "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
    }
    out = {
        "format": FORMAT,
        "import_tag": args.tag,
        "episode_id": episode_id(rel, text),
        "source": SOURCE,
        "agent": args.agent,
        "place": {"kind": "file", "name": rel, "id": None},
        "as_of": {"start": when, "end": when},
        "labels": {
            "sensitivity": args.sensitivity,
            "partner": None,
            "topic": topics_of(rel),
            "book_hint": None,
            "credential_redacted": False,
        },
        "summary": None,
        "messages": [message],
    }
    out["hash"] = canonical_hash(out)
    return out


def walk(root, exts):
    """Every candidate file under root, relative path and absolute, sorted.

    A dotted directory is not descended into and a dotted file is not taken: an
    agent's workspace keeps its own machinery in those, and none of it is a note.
    """
    for here, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if not d.startswith("."))
        for f in sorted(files):
            if f.startswith("."):
                continue
            ext = f.rsplit(".", 1)[-1].lower() if "." in f else ""
            if ext not in exts:
                continue
            full = os.path.join(here, f)
            yield os.path.relpath(full, root).replace(os.sep, "/"), full


def read_note(full, max_bytes, counts):
    """A note's text and mtime, or None with the reason counted."""
    try:
        size = os.path.getsize(full)
    except OSError:
        counts["unreadable"] += 1
        return None
    if size > max_bytes:
        counts["too_big"] += 1
        return None
    try:
        with open(full, "rb") as fh:
            raw = fh.read()
        text = raw.decode("utf-8")
    except (OSError, UnicodeDecodeError):
        counts["unreadable"] += 1
        return None
    if not text.strip():
        counts["empty"] += 1
        return None
    return text, int(os.path.getmtime(full) * 1000)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", required=True, help="the notes directory to walk")
    ap.add_argument("--tag", required=True, help="the import tag (import erase --tag takes it back)")
    ap.add_argument("--agent", default=None, help="the agent whose notes these are")
    ap.add_argument("--author", default=None, help="each message's author (default agent:<agent>, else `agent`)")
    ap.add_argument(
        "--integrity",
        default="agent",
        choices=["agent", "operator"],
        help="whose writing it is: `agent` for an agent's own notes, `operator` for the owner's own lines",
    )
    ap.add_argument(
        "--sensitivity",
        default="company-confidential",
        choices=["personal", "company-confidential", "partner-confidential", "public"],
    )
    ap.add_argument("--ext", default=DEFAULT_EXT, help="extensions to take, comma separated")
    ap.add_argument("--max-bytes", type=int, default=DEFAULT_MAX_BYTES)
    ap.add_argument("--limit", type=int, default=0, help="stop after this many notes (0: every one)")
    args = ap.parse_args(argv)
    if args.author is None:
        args.author = ("agent:" + args.agent) if args.agent else "agent"
    exts = {e.strip().lower().lstrip(".") for e in args.ext.split(",") if e.strip()}
    if not exts:
        print("notes-export: --ext named no extension", file=sys.stderr)
        return 2
    if not os.path.isdir(args.root):
        print("notes-export: %s is no directory" % args.root, file=sys.stderr)
        return 2

    counts = {"written": 0, "too_big": 0, "empty": 0, "unreadable": 0, "secret_name": 0}
    out = sys.stdout
    for rel, full in walk(args.root, exts):
        if looks_like_a_secret(os.path.basename(rel)):
            counts["secret_name"] += 1
            continue
        got = read_note(full, args.max_bytes, counts)
        if got is None:
            continue
        text, mtime_ms = got
        out.write(json.dumps(episode_of(rel, text, mtime_ms, args), sort_keys=True))
        out.write("\n")
        counts["written"] += 1
        if args.limit and counts["written"] >= args.limit:
            break
    out.flush()
    print(
        "notes-export: %d notes written; left out: %d over --max-bytes, %d empty, "
        "%d unreadable, %d named as a secret store"
        % (
            counts["written"],
            counts["too_big"],
            counts["empty"],
            counts["unreadable"],
            counts["secret_name"],
        ),
        file=sys.stderr,
    )
    return 0 if counts["written"] else 3


if __name__ == "__main__":
    sys.exit(main())

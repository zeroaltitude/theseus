#!/usr/bin/env python3
"""rows-export.py: the Beads rows and the GitHub issues, as episodes for Theseus's import.

Reads JSON that another tool produced and writes episode lines (format 1:
crates/theseus-core/src/import/episode.rs) to stdout, one episode per row or issue, for
`theseus import openclaw -`. Python's standard library only. **It runs nothing and
reaches no network**: the caller runs `bd` or `gh` and hands the JSON over, so this
script cannot create, change or close anything, on GitHub or anywhere else.

    bd list --json > rows.json  &&  scripts/rows-export.py --beads rows.json  --tag beads-2026-11
    gh issue list --json number,title,state,labels,author,createdAt,updatedAt,body \\
        --limit 1000 > issues.json
    scripts/rows-export.py --issues issues.json --repo owner/name --tag issues-2026-11

This is source (3) of `b4-theseus-holds-its-own-notes` (row B4): the tracker's own rows
in Theseus's memory, so a question about what is open on a row is answered from the store
with a citation. The GitHub half exists for when the issues exist; nothing here creates
them.

ONE ROW IS ONE EPISODE:

  source       `beads` for a Beads row, `github-issues` for an issue.
  episode_id   `ep_` + sha256 of the source, the row's id and its RENDERED TEXT,
               NUL-joined. The text is in the id, as it is for an agent's notes: a row
               whose state moved is a NEW episode and the one it replaces stays as
               history, so the record only grows and nothing has to decide whether a
               change is an append. A row that did not change re-exports to the same id
               and the same hash, and the import skips it, so a nightly run writes only
               what moved.
  place        `{"kind": "file", "name": "beads/<id>"}` or `{"kind": "file", "name":
               "<repo>#<number>"}`. A row is a record, not a conversation.
  as_of        the row's updated time, start and end alike (its created time when it
               names no updated one, else the run's own time).
  labels       sensitivity from --sensitivity (`company-confidential` by default), and
               the row's own labels as topic labels.
  messages     the row, rendered as one message; for an issue, one more message per
               comment when the JSON carries them. A login in --owner-login is the
               owner's own (`operator`); every other person's is outside text
               (`outside`, author `person:<login>`), which the index marks external.
               The owner's logins are the owner's, so they are named on the command
               line and never in this repository.

A ROW IS DURABLE UNTIL IT IS RETIRED, so the durable-versus-run-state rule
(docs/design/agent-notes-rule.md) does not judge these: it reads notes, and a row is not
a note. `--include-retired` is OFF by default, so a closed or retired row is counted and
left out until the owner says otherwise.

The render is deterministic: the known fields in a fixed order, then every other scalar
field in sorted key order. That matters because the episode's id is the render's digest,
so two runs over an unchanged row must produce the same bytes.
"""

import argparse
import hashlib
import json
import os
import sys
import time

BEADS_SOURCE = "beads"
ISSUES_SOURCE = "github-issues"
FORMAT = 1

#: The fields rendered first, in this order. Each name is tried in turn and the
#: first one the row carries wins, so one reader serves `bd`'s names and `gh`'s.
HEAD_FIELDS = (
    ("title", ("title",)),
    ("status", ("status", "state")),
    ("priority", ("priority",)),
    ("kind", ("kind", "issue_type", "type")),
    ("owner", ("owner", "assignee", "assignees")),
    ("author", ("author", "reporter", "created_by")),
    ("created", ("created", "created_at", "createdAt")),
    ("updated", ("updated", "updated_at", "updatedAt")),
    ("closed", ("closed", "closed_at", "closedAt")),
    ("labels", ("labels", "tags")),
    ("blocked_by", ("blocked_by", "blockedBy", "depends_on", "dependsOn")),
)

#: The long fields, rendered as their own sections after the head, in this order.
BODY_FIELDS = (
    ("Description", ("description", "body", "design")),
    ("Acceptance", ("acceptance", "acceptance_criteria")),
    ("Notes", ("notes",)),
)

#: What a row's own id is called.
ID_FIELDS = ("id", "number", "key", "slug")

#: What says a row is retired, by its status, in any case.
RETIRED = ("closed", "retired", "done", "cancelled", "canceled", "wont_fix", "wontfix")


def canonical_hash(v):
    s = json.dumps(v, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def episode_id(source, row_id, text):
    h = hashlib.sha256("\0".join([source, row_id, text]).encode("utf-8"))
    return "ep_" + h.hexdigest()


def first(row, names):
    for n in names:
        if n in row and row[n] not in (None, "", [], {}):
            return row[n]
    return None


def flat(v):
    """One scalar, a list of scalars, or a dict with a name, as one line's value."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float, str)):
        return str(v).strip()
    if isinstance(v, dict):
        named = first(v, ("login", "name", "title", "id"))
        return flat(named) if named is not None else ""
    if isinstance(v, list):
        parts = [flat(x) for x in v]
        return ", ".join(p for p in parts if p)
    return ""


def labels_of(row):
    v = first(row, ("labels", "tags"))
    if v is None:
        return []
    if isinstance(v, str):
        return [p.strip() for p in v.split(",") if p.strip()]
    out = []
    for x in v if isinstance(v, list) else []:
        s = flat(x)
        if s:
            out.append(s)
    return out


def row_id_of(row):
    v = first(row, ID_FIELDS)
    return flat(v) if v is not None else ""


def retired(row):
    status = flat(first(row, ("status", "state")) or "").lower().replace(" ", "_")
    return status in RETIRED


def render(row):
    """The row as one document. Deterministic: the head fields in their order,
    then the body sections, then every other scalar field sorted by key."""
    seen = set(ID_FIELDS)
    lines = []
    for label, names in HEAD_FIELDS:
        for n in names:
            seen.add(n)
        v = first(row, names)
        if v is not None:
            s = flat(v)
            if s:
                lines.append("%s: %s" % (label, s))
    rest = []
    for k in sorted(row):
        if k in seen:
            continue
        if any(k in names for _, names in BODY_FIELDS):
            continue
        s = flat(row[k])
        if s and len(s) <= 200:
            rest.append("%s: %s" % (k, s))
    lines.extend(rest)
    out = "\n".join(lines)
    for label, names in BODY_FIELDS:
        for n in names:
            seen.add(n)
        v = first(row, names)
        if v is None:
            continue
        s = v if isinstance(v, str) else flat(v)
        if s.strip():
            out += "\n\n## %s\n\n%s" % (label, s.strip())
    return out.strip() + "\n"


def message(idx, when, author, integrity, text, unit):
    return {
        "idx": idx,
        "time": when,
        "author": author,
        "integrity": integrity,
        "text": text,
        "unit": unit,
        "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
    }


def iso_of(v, fallback):
    """A time as the format wants it, or `fallback` when it cannot be read."""
    s = flat(v) if v is not None else ""
    if not s:
        return fallback
    s = s.replace(" ", "T")
    if s.endswith("Z"):
        return s
    if "+" in s[10:] or s.endswith("UTC"):
        return s.split("+")[0].rstrip("UTC").rstrip() + "Z"
    if "T" not in s:
        return s + "T00:00:00Z"
    return s + "Z"


def comments_of(row, args, when, unit):
    """An issue's comments, each its own message after the row's."""
    out = []
    v = first(row, ("comments",))
    if not isinstance(v, list):
        return out
    for i, c in enumerate(v):
        if not isinstance(c, dict):
            continue
        text = c.get("body") or c.get("text") or ""
        if not str(text).strip():
            continue
        login = flat(first(c, ("author", "user", "login"))) or "unknown"
        owner = login in args.owner_login
        out.append(
            message(
                i + 1,
                iso_of(first(c, ("createdAt", "created_at", "created")), when),
                "operator" if owner else "person:" + login,
                "operator" if owner else "outside",
                str(text),
                "%s#c%d" % (unit, i + 1),
            )
        )
    return out


def episode_of(row, source, args, now_iso):
    rid = row_id_of(row)
    if not rid:
        return None, "no id"
    text = render(row)
    if source == ISSUES_SOURCE:
        name = ("%s#%s" % (args.repo, rid)) if args.repo else ("issue#%s" % rid)
        unit = name
    else:
        name = "beads/%s" % rid
        unit = name
    when = iso_of(
        first(row, ("updated", "updated_at", "updatedAt"))
        or first(row, ("created", "created_at", "createdAt")),
        now_iso,
    )
    author = flat(first(row, ("author", "reporter", "created_by")))
    if source == BEADS_SOURCE:
        # Our own tracker is the owner's own configuration, as a scheduled
        # prompt is in `openclaw-export.py`: the row is `operator`, author
        # `beads`, whoever typed it.
        head = message(0, when, "beads", "operator", text, unit)
        messages = [head]
    else:
        owner = author in args.owner_login
        head = message(
            0,
            when,
            "operator" if owner else ("person:" + author if author else "outside"),
            "operator" if owner else "outside",
            text,
            unit,
        )
        messages = [head] + comments_of(row, args, when, unit)
    out = {
        "format": FORMAT,
        "import_tag": args.tag,
        "episode_id": episode_id(source, rid, text),
        "source": source,
        "agent": None,
        "place": {"kind": "file", "name": name, "id": rid},
        "as_of": {"start": messages[0]["time"], "end": messages[-1]["time"]},
        "labels": {
            "sensitivity": args.sensitivity,
            "partner": None,
            "topic": labels_of(row),
            "book_hint": None,
            "credential_redacted": False,
        },
        "summary": None,
        "messages": messages,
    }
    out["hash"] = canonical_hash(out)
    return out, None


def rows_of(doc):
    """The rows of whatever shape the tool wrote: a list, or an object holding
    one under a name we know."""
    if isinstance(doc, list):
        return [r for r in doc if isinstance(r, dict)]
    if isinstance(doc, dict):
        for k in ("issues", "rows", "items", "beads", "data", "results"):
            v = doc.get(k)
            if isinstance(v, list):
                return [r for r in v if isinstance(r, dict)]
    return []


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--beads", help="JSON of Beads rows (`-` for stdin)")
    ap.add_argument("--issues", help="JSON of GitHub issues (`-` for stdin)")
    ap.add_argument("--repo", default=None, help="owner/name, for an issue's place name")
    ap.add_argument("--tag", required=True, help="the import tag (import erase --tag takes it back)")
    ap.add_argument(
        "--owner-login",
        action="append",
        default=[],
        help="a login whose text is the owner's own (operator); repeat it. Any other person's is outside text",
    )
    ap.add_argument(
        "--include-retired",
        action="store_true",
        help="take a closed or retired row too (OFF by default: a row is durable until it is retired)",
    )
    ap.add_argument(
        "--sensitivity",
        default="company-confidential",
        choices=["personal", "company-confidential", "partner-confidential", "public"],
    )
    args = ap.parse_args(argv)
    if not args.beads and not args.issues:
        print("rows-export: name --beads or --issues", file=sys.stderr)
        return 2

    now_iso = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
    counts = {"written": 0, "retired": 0, "no_id": 0, "empty": 0}
    out = sys.stdout
    for path, source in ((args.beads, BEADS_SOURCE), (args.issues, ISSUES_SOURCE)):
        if not path:
            continue
        try:
            text = sys.stdin.read() if path == "-" else open(path, encoding="utf-8").read()
            doc = json.loads(text)
        except (OSError, ValueError) as e:
            print("rows-export: %s is not readable JSON: %s" % (path, e), file=sys.stderr)
            return 2
        rows = rows_of(doc)
        if not rows:
            print("rows-export: %s held no row" % path, file=sys.stderr)
            continue
        for row in rows:
            if retired(row) and not args.include_retired:
                counts["retired"] += 1
                continue
            ep, why = episode_of(row, source, args, now_iso)
            if ep is None:
                counts["no_id" if why == "no id" else "empty"] += 1
                continue
            out.write(json.dumps(ep, sort_keys=True))
            out.write("\n")
            counts["written"] += 1
    out.flush()
    print(
        "rows-export: %d rows written; left out: %d retired (--include-retired takes them), "
        "%d with no id" % (counts["written"], counts["retired"], counts["no_id"]),
        file=sys.stderr,
    )
    return 0 if counts["written"] else 3


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""rows-export.py's own unit tests: the render, the id, the hash, and what it leaves out.

The rows and issues here are invented. Nothing in this file or in the script reaches a
network or runs another program: the caller hands the JSON over, which is what makes the
GitHub half a read path and not a write one.
"""

import hashlib
import importlib.util
import io
import json
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "rows_export", os.path.join(HERE, "..", "rows-export.py")
)
rows_export = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rows_export)

ROWS = [
    {
        "id": "theseus-da9hv",
        "title": "Theseus holds its operating notes",
        "status": "open",
        "priority": 1,
        "labels": ["memory", "import"],
        "description": "Everything an agent needs in order to work on Theseus.",
        "notes": "Design row first, then the import.",
        "created": "2026-10-30T11:00:00Z",
        "updated": "2026-11-12T20:58:24Z",
    },
    {"id": "theseus-shut", "title": "A finished thing", "status": "closed"},
]

ISSUES = [
    {
        "number": 41,
        "title": "The gate reds on a four-core box",
        "state": "OPEN",
        "labels": [{"name": "gate"}],
        "author": {"login": "zeroaltitude"},
        "createdAt": "2026-11-01T09:00:00Z",
        "updatedAt": "2026-11-02T09:00:00Z",
        "body": "Thirty-six tests fail under root.",
        "comments": [
            {"author": {"login": "wrenbuilder"}, "body": "It is RLIMIT_NPROC.", "createdAt": "2026-11-02T10:00:00Z"},
            {"author": {"login": "zeroaltitude"}, "body": "Then say so in the brief.", "createdAt": "2026-11-02T11:00:00Z"},
        ],
    }
]


def run(doc, *extra, which="--beads"):
    """The exporter's lines and its exit status, the JSON given on stdin."""
    out, old_out = io.StringIO(), sys.stdout
    old_in = sys.stdin
    sys.stdout, sys.stdin = out, io.StringIO(json.dumps(doc))
    try:
        rc = rows_export.main([which, "-", "--tag", "rows-test"] + list(extra))
    finally:
        sys.stdout, sys.stdin = old_out, old_in
    return [json.loads(l) for l in out.getvalue().splitlines() if l.strip()], rc


class RowsExport(unittest.TestCase):
    def test_a_beads_row_renders_once_and_the_retired_one_is_left_out(self):
        lines, rc = run(ROWS)
        self.assertEqual(rc, 0)
        # The positive count beside the zero: one row in, one left out.
        self.assertEqual([e["place"]["name"] for e in lines], ["beads/theseus-da9hv"])
        e = lines[0]
        self.assertEqual(e["source"], "beads")
        self.assertEqual(e["place"]["kind"], "file")
        self.assertEqual(e["labels"]["topic"], ["memory", "import"])
        self.assertEqual(e["as_of"]["start"], "2026-11-12T20:58:24Z")
        self.assertEqual(len(e["messages"]), 1)
        m = e["messages"][0]
        self.assertEqual((m["author"], m["integrity"]), ("beads", "operator"))
        text = m["text"]
        self.assertTrue(text.startswith("title: Theseus holds its operating notes\n"), text)
        self.assertIn("status: open", text)
        self.assertIn("## Description", text)
        self.assertIn("## Notes", text)

    def test_include_retired_takes_the_closed_row_too(self):
        lines, rc = run(ROWS, "--include-retired")
        self.assertEqual(rc, 0)
        self.assertEqual(
            [e["place"]["name"] for e in lines],
            ["beads/theseus-da9hv", "beads/theseus-shut"],
        )

    def test_the_render_is_deterministic_and_the_id_is_its_digest(self):
        first, _ = run(ROWS)
        again, _ = run(ROWS)
        self.assertEqual(first[0]["episode_id"], again[0]["episode_id"])
        self.assertEqual(first[0]["messages"][0]["text"], again[0]["messages"][0]["text"])
        want = hashlib.sha256(
            "\0".join(["beads", "theseus-da9hv", first[0]["messages"][0]["text"]]).encode("utf-8")
        ).hexdigest()
        self.assertEqual(first[0]["episode_id"], "ep_" + want)
        # A row whose state moved is a NEW episode; the one it replaces stays.
        moved = [dict(ROWS[0], status="in_progress"), ROWS[1]]
        other, _ = run(moved)
        self.assertNotEqual(first[0]["episode_id"], other[0]["episode_id"])

    def test_the_hash_is_the_canonical_json_of_everything_but_itself(self):
        lines, _ = run(ROWS)
        got = dict(lines[0])
        want = got.pop("hash")
        s = json.dumps(got, sort_keys=True, separators=(",", ":"))
        self.assertEqual(want, hashlib.sha256(s.encode("utf-8")).hexdigest())

    def test_an_issues_comments_are_messages_and_only_the_owners_are_the_owners(self):
        lines, rc = run(ISSUES, "--repo", "owner/name", "--owner-login", "zeroaltitude", which="--issues")
        self.assertEqual(rc, 0)
        e = lines[0]
        self.assertEqual(e["source"], "github-issues")
        self.assertEqual(e["place"]["name"], "owner/name#41")
        self.assertEqual([m["integrity"] for m in e["messages"]], ["operator", "outside", "operator"])
        self.assertEqual(
            [m["author"] for m in e["messages"]], ["operator", "person:wrenbuilder", "operator"]
        )
        self.assertEqual([m["idx"] for m in e["messages"]], [0, 1, 2])
        self.assertEqual(e["as_of"]["end"], "2026-11-02T11:00:00Z")
        self.assertEqual(e["labels"]["topic"], ["gate"])

    def test_without_an_owner_login_every_person_is_outside_text(self):
        lines, _ = run(ISSUES, which="--issues")
        e = lines[0]
        self.assertEqual(set(m["integrity"] for m in e["messages"]), {"outside"})

    def test_rows_come_out_of_whatever_wrapper_the_tool_wrote(self):
        for wrapper in ("issues", "rows", "items", "data", "results"):
            lines, _ = run({wrapper: ROWS, "total": 2})
            self.assertEqual(len(lines), 1, wrapper)

    def test_nothing_to_write_and_bad_input_are_told_apart(self):
        self.assertEqual(run([])[1], 3)
        self.assertEqual(run([{"title": "no id at all"}])[1], 3)
        out, old = io.StringIO(), sys.stdout
        sys.stdout = out
        try:
            rc = rows_export.main(["--tag", "rows-test"])
        finally:
            sys.stdout = old
        self.assertEqual(rc, 2, "naming neither source is a usage error")


if __name__ == "__main__":
    unittest.main()

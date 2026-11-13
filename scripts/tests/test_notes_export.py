#!/usr/bin/env python3
"""notes-export.py's own unit tests: the walk, the id, the hash and what it leaves out.

Each case builds a throwaway notes directory, runs the exporter in-process, and reads
the episode lines back. Nothing here classifies a note: the durable-versus-run-state
rule has one implementation and it is the daemon's (crates/theseus-core/src/import/notes.rs).
"""

import hashlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "notes_export", os.path.join(HERE, "..", "notes-export.py")
)
notes_export = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notes_export)

RULES = (
    "- The gate runs before every commit, and you chain it with `&&`.\n"
    "- Every commit subject names its issue id in parentheses.\n"
)
LOG = "Ran the suite after the rebase. Pushed it.\n"


def write(root, rel, text):
    full = os.path.join(root, rel)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", encoding="utf-8") as fh:
        fh.write(text)
    return full


def run(root, *extra):
    """The exporter's lines and its exit status."""
    out = io.StringIO()
    old = sys.stdout
    sys.stdout = out
    try:
        rc = notes_export.main(["--root", root, "--tag", "notes-test"] + list(extra))
    finally:
        sys.stdout = old
    lines = [json.loads(l) for l in out.getvalue().splitlines() if l.strip()]
    return lines, rc


class NotesExport(unittest.TestCase):
    def test_one_note_is_one_episode_named_by_its_relative_path(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "refs/building.md", RULES)
            write(d, "log/day.md", LOG)
            lines, rc = run(d, "--agent", "wren")
            self.assertEqual(rc, 0)
            self.assertEqual([e["place"]["name"] for e in lines], ["log/day.md", "refs/building.md"])
            for e in lines:
                self.assertEqual(e["source"], "agent-notes")
                self.assertEqual(e["place"]["kind"], "file")
                self.assertEqual(len(e["messages"]), 1)
                self.assertEqual(e["messages"][0]["author"], "agent:wren")
                self.assertEqual(e["messages"][0]["integrity"], "agent")
                self.assertEqual(e["as_of"]["start"], e["as_of"]["end"])
            self.assertEqual(lines[1]["messages"][0]["text"], RULES)
            self.assertEqual(lines[1]["labels"]["topic"], ["refs"])
            self.assertEqual(lines[0]["labels"]["topic"], ["log"])

    def test_the_hash_is_the_canonical_json_of_everything_but_itself(self):
        # The daemon checks exactly this (import/episode.rs: `hash_matches`).
        with tempfile.TemporaryDirectory() as d:
            write(d, "refs/building.md", RULES)
            lines, _ = run(d)
            e = lines[0]
            got = dict(e)
            want = got.pop("hash")
            s = json.dumps(got, sort_keys=True, separators=(",", ":"))
            self.assertEqual(want, hashlib.sha256(s.encode("utf-8")).hexdigest())
            self.assertEqual(
                e["messages"][0]["sha256"],
                hashlib.sha256(RULES.encode("utf-8")).hexdigest(),
            )

    def test_an_edited_note_is_a_new_episode_and_an_unedited_one_is_the_same(self):
        # The text is in the id, so an edit never overwrites what is held.
        with tempfile.TemporaryDirectory() as d:
            write(d, "refs/building.md", RULES)
            first, _ = run(d)
            again, _ = run(d)
            self.assertEqual(first[0]["episode_id"], again[0]["episode_id"])
            write(d, "refs/building.md", RULES + "- Never raise a ceiling to buy a green gate.\n")
            edited, _ = run(d)
            self.assertNotEqual(first[0]["episode_id"], edited[0]["episode_id"])
            self.assertTrue(edited[0]["episode_id"].startswith("ep_"))
            self.assertEqual(len(edited[0]["episode_id"]), 3 + 64)

    def test_what_it_leaves_out(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "refs/building.md", RULES)          # taken
            write(d, "refs/api_token.md", "x" * 40)      # a secret by its name
            write(d, "refs/deploy.pem", "x" * 40)        # a secret by its suffix
            write(d, "refs/notes.bin", "x" * 40)         # not an --ext
            write(d, "refs/empty.md", "   \n")           # empty
            write(d, ".cache/old.md", RULES)             # a dotted directory
            write(d, "refs/.hidden.md", RULES)           # a dotted file
            write(d, "refs/huge.md", "y" * 200)          # over --max-bytes below
            lines, rc = run(d, "--max-bytes", "150")
            self.assertEqual(rc, 0)
            # The positive count beside the zero: exactly the one note is left.
            self.assertEqual([e["place"]["name"] for e in lines], ["refs/building.md"])

    def test_a_directory_with_no_note_exits_three_and_writes_nothing(self):
        with tempfile.TemporaryDirectory() as d:
            lines, rc = run(d)
            self.assertEqual(lines, [])
            self.assertEqual(rc, 3)

    def test_the_owners_own_lines_can_come_in_as_the_operators(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "decisions.md", RULES)
            lines, _ = run(d, "--integrity", "operator", "--author", "owner")
            self.assertEqual(lines[0]["messages"][0]["integrity"], "operator")
            self.assertEqual(lines[0]["messages"][0]["author"], "owner")
            self.assertEqual(lines[0]["labels"]["topic"], [])

    def test_a_bad_root_or_no_extension_is_a_usage_error(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(run(os.path.join(d, "gone"))[1], 2)
            self.assertEqual(run(d, "--ext", " ")[1], 2)


if __name__ == "__main__":
    unittest.main()

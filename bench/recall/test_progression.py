"""The format's tests: the check language, the buckets, the validator, and
the workspace. Standard library only:

    python3 -m unittest discover -s bench/recall
"""

from __future__ import annotations

import copy
import hashlib
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import checks  # noqa: E402
import progression as pg  # noqa: E402


class CheckLanguage(unittest.TestCase):
    def test_strings_words_and_regexes_as_check_rs_reads_them(self):
        for check, reply, want in [
            ('reply has "7433"', "It listens on 7433.", True),
            ('reply has "gate.sh && git"', "run gate.sh\n  &&   git commit", True),
            ('reply has "don\'t"', "I don’t know", True),
            ('reply lacks "fjall" | "sled"', "redb it is", True),
            ('reply lacks "fjall" | "sled"', "sled, then", False),
            ('reply has word "redb"', "the redb store", True),
            ('reply has word "redb"', "the redbird", False),
            ('reply has word "nov 3"', "on Nov 30", False),
            ('reply has word "nov 3"', "on Nov 3.", True),
            (r"reply has /\bmv\s+-f\b/", "mv  -f a b", True),
            (r"reply has /\bMV\b/i", "mv a b", True),
            (r"reply has /\bMV\b/", "mv a b", False),
            ('reply has "x" or reply has "y"', "y", True),
            (r"reply has /\/srv\/a/", "at /srv/a/b", True),
        ]:
            self.assertEqual(checks.passes(check, reply), want, (check, reply))

    def test_every_line_must_hold(self):
        c = 'reply has "a"\n# a comment\n\nreply lacks "b"'
        self.assertTrue(checks.passes(c, "a"))
        self.assertFalse(checks.passes(c, "a b"))
        self.assertEqual([ok for _, ok in checks.run(c, "a b")], [True, False])

    def test_files_are_read_under_the_root(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "notes").mkdir()
            (root / "notes" / "x.txt").write_text("http://127.0.0.1:23817/healthz\n")
            self.assertTrue(checks.passes('file "notes/x.txt" has word "23817"', "", root))
            self.assertTrue(checks.passes('file "notes/x.txt" exists', "", root))
            self.assertTrue(checks.passes('file "notes/y.txt" absent', "", root))
            self.assertFalse(checks.passes('file "notes/y.txt" has "x"', "", root))
            self.assertTrue(checks.passes('file "notes/y.txt" lacks "x"', "", root))
            # No root: every file is unread.
            self.assertFalse(checks.passes('file "notes/x.txt" has "x"', "", None))

    def test_parsing_is_strict(self):
        for bad in [
            "",
            "# only a comment",
            'reply contains "x"',
            'calls has "x"',
            'reply has "x',
            "reply has /x",
            'reply has "x" "y"',
            'reply has "x" |',
            'file "../x" has "y"',
            'file "/etc/x" has "y"',
            'reply exists',
            'file "x" exists "y"',
            "reply has /(/",
            'reply has /x/q',
            'reply has "\\q"',
        ]:
            with self.assertRaises(checks.CheckError, msg=bad):
                checks.parse(bad)


def tiny() -> pg.Progression:
    """Two sessions of six turns, one fact in each, its probe after it, and a
    mark at turn 3."""
    sessions = [pg.Session("session 1", "2026-11-02", "Monday"), pg.Session("session 2", "2026-11-05", "Thursday")]
    turns = []
    roles = ["opener", "fact", "filler", "filler", "filler", "probe",
             "opener", "fact", "probe", "filler", "filler", "probe"]
    texts = {1: "Run ./scripts/check-alder-relay.sh: did the build pass?",
             7: "Side note: the birch mailer is on port 31555 these days."}
    for i, r in enumerate(roles):
        turns.append(pg.Turn(index=i, session=i // 6, block=i // 3, topic="t", text=texts.get(i, f"turn {i}"),
                             role=r, est_tokens=100, mark=(i == 3)))
    turns[3].role = "bulk"
    facts = [
        pg.Fact("f001", "the alder relay's port", "port", "23817", "incidental", "tool_output", "output", 1,
                "scripts/check-alder-relay.sh", "23817"),
        pg.Fact("f002", "the birch mailer's port", "port", "31555", "central", "said", "topic", 7, "said", "31555"),
    ]
    probes = [
        pg.Probe("p001", "f001", "direct", "compaction", "incidental", "port", 5,
                 "Which port is the alder relay on?", pg.value_check("reply", "port", "23817", None), "f001",
                 "the alder relay's port"),
        pg.Probe("p002", "f002", "indirect", "near", "central", "port", 8,
                 "Write the birch mailer's URL into notes/p002.txt.",
                 pg.value_check('file "notes/p002.txt"', "port", "31555", None), "f002",
                 "the birch mailer's port", file="notes/p002.txt"),
        pg.Probe("p003", None, "abstention", "days", "incidental", "port", 11,
                 "Which port is the cedar relay on?", pg.kind_check("port"), "f001", "the cedar relay's port",
                 distractors=["f001"]),
    ]
    return pg.Progression(pg.FORMAT, 1, "smoke", sessions, turns, facts, probes,
                          {"README.md": {"content": "x\n", "executable": False},
                           "scripts/check-alder-relay.sh": {"content": "#!/bin/sh\necho 23817\n", "executable": True}},
                          32000)


class Format(unittest.TestCase):
    def test_a_tiny_progression_is_valid_and_round_trips(self):
        p = tiny()
        pg.validate(p)
        with tempfile.TemporaryDirectory() as d:
            p.save(Path(d))
            q = pg.load(Path(d))
        self.assertEqual(q.digest(), p.digest())
        self.assertEqual(q.to_json(), p.to_json())

    def test_the_planned_overhead_is_kept_and_an_older_file_without_it_loads(self):
        """`overhead_tokens` (theseus-dp3y) round-trips; a file from before it
        loads with None, and keeps its digest: the key is written only when
        set, so its canonical bytes are what they were."""
        p = tiny()
        self.assertIsNone(p.overhead_tokens)
        self.assertNotIn("overhead_tokens", p.to_json())
        before = hashlib.sha256(json.dumps(p.to_json(), sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        with tempfile.TemporaryDirectory() as d:
            p.save(Path(d))
            q = pg.load(Path(d))
        self.assertIsNone(q.overhead_tokens)
        self.assertEqual(q.digest(), before)
        p.overhead_tokens = 13650
        self.assertEqual(p.to_json()["overhead_tokens"], 13650)
        self.assertNotEqual(p.digest(), before)
        with tempfile.TemporaryDirectory() as d:
            p.save(Path(d))
            q = pg.load(Path(d))
        self.assertEqual((q.overhead_tokens, q.digest()), (13650, p.digest()))

    def test_a_fact_probed_twice_is_refused(self):
        p = tiny()
        p.probes.append(copy.deepcopy(p.probes[1]))
        p.probes[-1].id, p.probes[-1].turn, p.probes[-1].kind = "p004", 11, "direct"
        p.probes[-1].bucket, p.probes[-1].check = "topic_shift", pg.value_check("reply", "port", "31555", None)
        p.probes[2].turn = 10
        p.turns[10].role = "probe"
        with self.assertRaisesRegex(pg.Invalid, "probed twice"):
            pg.validate(p)

    def test_a_probe_before_its_fact_is_refused(self):
        p = tiny()
        p.facts[0].turn = 5
        with self.assertRaisesRegex(pg.Invalid, "not after"):
            pg.validate(p)

    def test_a_value_said_outside_its_turn_is_refused(self):
        p = tiny()
        p.turns[2].text = "Is 23817 free?"
        with self.assertRaisesRegex(pg.Invalid, "23817"):
            pg.validate(p)

    def test_a_bucket_the_marks_contradict_is_refused(self):
        p = tiny()
        p.probes[0].bucket = "near"
        with self.assertRaisesRegex(pg.Invalid, "marks make it compaction"):
            pg.validate(p)

    def test_buckets_follow_where_the_arm_compacted(self):
        p = tiny()
        # As planned: the mark at turn 3 lies between turn 1 and turn 5.
        self.assertEqual(pg.bucket_of(p, 1, 5, p.marks(), False), "compaction")
        # An arm that compacted at turn 5 itself: its request for the probe
        # was compacted, so the fact was gone before it.
        self.assertEqual(pg.bucket_of(p, 1, 5, [5], False), "compaction")
        # One that never compacted, or compacted only after the probe.
        self.assertEqual(pg.bucket_of(p, 1, 5, [], False), "topic_shift")
        self.assertEqual(pg.bucket_of(p, 1, 5, [6], False), "topic_shift")
        self.assertEqual(pg.bucket_of(p, 1, 2, [], False), "near")
        self.assertEqual(pg.bucket_of(p, 1, 9, [], False), "days")
        self.assertEqual(pg.bucket_of(p, 1, 9, [], True), "supersession")

    def test_the_expected_cells_leave_out_an_abstentions_supersession(self):
        cells = pg.expected_cells()
        self.assertEqual(len(cells), 34)
        self.assertNotIn(("supersession", "central", "abstention"), cells)

    def test_a_date_is_found_in_the_forms_people_write(self):
        c = pg.value_check("reply", "date", "2026-11-03", None)
        for reply, want in [
            ("2026-11-03", True),
            ("On November 3, the queue stalled.", True),
            ("nov 3", True),
            ("3 Nov", True),
            ("Nov 30", False),
            ("2026-11-04", False),
            ("on November 3rd", True),
            ("the 3rd of November", True),
            ("Nov 23rd", False),
        ]:
            self.assertEqual(checks.passes(c, reply), want, reply)

    def test_the_abstention_check_wants_no_value_and_an_admission(self):
        c = pg.kind_check("port")
        for reply, want in [
            ("I don't know which port the cedar relay uses; it was never mentioned.", True),
            ("I have no record of the cedar relay's port.", True),
            ("It's 23817.", False),
            ("I don't know; maybe 23817?", False),
            ("Port 8080 is common.", False),
            ("I'm not aware of a cedar relay; it never came up.", True),
            ("Nothing in our conversation says.", True),
            ("It's 23817, not 23818.", False),
            # A date's year is not a port.
            ("As of 2026-11-05 I haven't seen it mentioned.", True),
        ]:
            self.assertEqual(checks.passes(c, reply), want, reply)
        self.assertTrue(checks.passes(pg.kind_check("path"), "I couldn't find it: no record under /srv."))
        self.assertFalse(checks.passes(pg.kind_check("path"), "Not sure: /srv/cedar/relay/spool-1.db?"))

    def test_a_plain_abstention_is_an_admission(self):
        """The live smoke's p003, its short forms, and replies that are not
        admissions (theseus-523y)."""
        c = pg.kind_check("version")
        live = ("I can't tell. A case-insensitive search for \"lanyard\" across the 20 workspace files found no "
                "matches, so nothing there pins a lanyard version for veery.")
        for reply, want in [
            (live, True),
            ("I can't tell.", True),
            ("No matches in the workspace.", True),
            ("A search found no lanyard pin.", True),
            ("Nothing in the files pins it.", True),
            ("Nothing here says which version.", True),
            ("Nothing in the workspace shows a pin for it.", True),
            ("I'm not able to find a pin for it.", True),
            ("I was unable to find it.", True),
            ("It's 6.13.15.", False),
            ("I can't tell for sure, but 6.13.15.", False),
            ("Sure, I can tell you: it's pinned in requirements.txt.", False),
            ("The lanyard pin matches the ratline one.", False),
            ("I found it pinned in setup.cfg.", False),
            ("It's pinned, as the lockfile shows.", False),
        ]:
            self.assertEqual(checks.passes(c, reply), want, reply)
        # Each is a hedge too: no confident-wrong for a value beside it.
        for reply in ("I can't tell", "no matches", "it found no pin", "unable to say", "not able to find it"):
            self.assertIsNotNone(re.search(pg.HEDGE, reply, re.I), reply)

    def test_a_version_may_carry_its_v(self):
        c = pg.value_check("reply", "version", "6.13.15", "6.12.0")
        self.assertTrue(checks.passes(c, "It pins v6.13.15."))
        self.assertTrue(checks.passes(c, "6.13.15"))
        self.assertFalse(checks.passes(c, "6.13.150"))
        self.assertFalse(checks.passes(c, "v6.13.15, up from v6.12.0"))

    def test_each_kinds_values_are_found(self):
        self.assertEqual(pg.find_values("port", "on 127.0.0.1:23817, not 2026-11-03"), ["23817"])
        self.assertEqual(pg.find_values("version", "pins 3.4.5, not 127.0.0.1"), ["3.4.5"])
        self.assertEqual(pg.find_values("host", "ssh deploy@flint-canary-42.lan"), ["flint-canary-42.lan"])
        self.assertEqual(pg.find_values("ticket", "fixes ALD-1234."), ["ALD-1234"])
        self.assertEqual(pg.find_values("path", "cp /srv/alder/relay/spool-41.db /tmp/"),
                         ["/srv/alder/relay/spool-41.db"])

    def test_the_workspace_is_written_and_changed_before_a_turn(self):
        p = tiny()
        p.turns[2].before = [pg.Edit("scripts/check-alder-relay.sh", "#!/bin/sh\necho ok\n", True),
                             pg.Edit("README.md", None)]
        with tempfile.TemporaryDirectory() as d:
            root = Path(d) / "ws"
            pg.materialize(p, root)
            self.assertTrue(os.access(root / "scripts/check-alder-relay.sh", os.X_OK))
            with self.assertRaises(FileExistsError):
                pg.materialize(p, root)
            pg.apply_before(p.turns[2], root)
            self.assertEqual((root / "scripts/check-alder-relay.sh").read_text(), "#!/bin/sh\necho ok\n")
            self.assertFalse((root / "README.md").exists())


if __name__ == "__main__":
    unittest.main()

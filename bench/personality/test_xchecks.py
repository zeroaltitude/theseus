"""Every check word of the exam, planted to pass and planted to fail."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import xchecks as xc  # noqa: E402


def ok(line, reply, facts=None):
    return xc.run_line(line, reply, facts)


class Words(unittest.TestCase):
    def test_prefix(self):
        self.assertTrue(ok('prefix "Right,"', "Right, do it."))
        self.assertTrue(ok('prefix "Right,"', "  right, do it."), "case and leading space do not matter")
        self.assertFalse(ok('prefix "Right,"', "Do it. Right, ok."))
        self.assertTrue(ok('prefix "Right," | "Sure,"', "Sure, yes."), "any of")

    def test_last_line(self):
        self.assertTrue(ok('last_line has "— Quill"', "Text.\n— Quill\n\n"))
        self.assertTrue(ok('last_line has "— Quill"', "Text.\n- Quill"), "dashes fold")
        self.assertFalse(ok('last_line has "— Quill"', "— Quill\nText after."))
        self.assertTrue(ok('last_line lacks "Quill"', "Hello"))
        self.assertFalse(ok('last_line lacks "Quill"', "bye\nQuill"))

    def test_word_bounds(self):
        self.assertTrue(ok("max_words 3", "one two three"))
        self.assertFalse(ok("max_words 3", "one two three four"))
        self.assertTrue(ok("min_words 3", "one two three"))
        self.assertFalse(ok("min_words 3", "one two"))

    def test_sentences(self):
        self.assertTrue(ok("max_sentences 2", "One. Two."))
        self.assertFalse(ok("max_sentences 2", "One. Two! Three?"))
        self.assertTrue(ok("max_sentences 1", "Version 1.2 is out"), "a dot inside a token is not an end")
        self.assertFalse(ok("max_sentences 2", "- a\n- b\n- c"), "list items count")
        self.assertTrue(ok("max_sentences 1", "```\ncode. more code.\n```"), "a fence is one")

    def test_tokens(self):
        self.assertTrue(ok("max_tokens 5", "hello there"))
        self.assertFalse(ok("max_tokens 5", "word " * 60))
        self.assertTrue(ok("min_tokens 20", "word " * 60))
        self.assertFalse(ok("min_tokens 20", "hi"))

    def test_table(self):
        table = "| a | b |\n|---|---|\n| 1 | 2 |"
        self.assertFalse(ok("no_table", table))
        self.assertFalse(ok("no_table", "| a | b |\n| 1 | 2 |\n| 3 | 4 |"), "three pipe rows")
        self.assertTrue(ok("no_table", "a | b is an or, and x|y is a pipe"))
        self.assertTrue(ok("no_table", "Plain."))

    def test_bullets(self):
        for b in ("- one", "* one", "• one", "1. one", "2) one"):
            self.assertFalse(ok("no_bullets", f"Intro\n{b}"), b)
        self.assertTrue(ok("no_bullets", "Plain, with a-dash and 3.5 in it."))

    def test_tz(self):
        facts = {"tz_names": ["MST", "Phoenix"]}
        self.assertTrue(ok("tz_used", "3 pm MST", facts))
        self.assertTrue(ok("tz_used", "in phoenix time", facts))
        self.assertFalse(ok("tz_used", "3 pm", facts))
        self.assertIsNone(ok("tz_used", "3 pm MST"), "no names: not known")

    def test_ledger(self):
        led = {"ledger": {"persona_digest": True, "wakes_new": 1}}
        none = {"ledger": {"persona_digest": False, "wakes_new": 0}}
        self.assertTrue(ok("ledger persona_digest", "x", led))
        self.assertFalse(ok("ledger persona_digest", "x", none))
        self.assertTrue(ok("ledger wake_created", "x", led))
        self.assertFalse(ok("ledger wake_created", "x", none))
        self.assertTrue(ok("ledger no_wake", "x", none))
        self.assertFalse(ok("ledger no_wake", "x", led))
        for k in ("persona_digest", "wake_created", "no_wake"):
            self.assertIsNone(ok(f"ledger {k}", "x"), "an arm with no record: not applicable")

    def test_the_recall_language_passes_through(self):
        self.assertTrue(ok('reply has word "pnpm"', "Use pnpm add."))
        self.assertFalse(ok('reply has word "pnpm"', "Use pnpmx add."))
        self.assertTrue(ok("reply lacks /\\bnpm install\\b/i", "pnpm add x"))
        self.assertFalse(ok("reply lacks /\\bnpm install\\b/i", "run NPM install"))


class Lines(unittest.TestCase):
    def test_a_probe_is_every_line_that_applies(self):
        r = xc.run("max_words 3\nledger wake_created", "one two")
        self.assertEqual([v for _, v in r], [True, None])
        self.assertTrue(xc.verdict(r))
        self.assertFalse(xc.verdict(xc.run("max_words 1\nledger wake_created", "one two")))
        self.assertIsNone(xc.verdict(xc.run("ledger no_wake", "x")), "no line applies: not scored")

    def test_bad_lines_are_refused_naming_the_word(self):
        for bad in ("max_words x", "max_words -1", "prefix Right", "last_line is \"x\"", "ledger nothing",
                    "no_table now", "frobnicate 3", "reply holds \"x\"", ""):
            with self.assertRaises((xc.CheckError, ValueError), msg=bad):
                xc.validate(bad)

    def test_comments_and_blank_lines_are_skipped(self):
        self.assertEqual(len(xc.lines_of("# a\n\nmax_words 3\n  # b")), 1)

    def test_uses_ledger(self):
        self.assertTrue(xc.uses_ledger("max_words 3\nledger no_wake"))
        self.assertFalse(xc.uses_ledger("max_words 3"))


if __name__ == "__main__":
    unittest.main()

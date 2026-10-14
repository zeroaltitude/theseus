import unittest

from textkit import count_words


class Holdout(unittest.TestCase):
    def test_other_inputs(self):
        self.assertEqual(count_words("   "), 0)
        self.assertEqual(count_words("\n\n"), 0)
        self.assertEqual(count_words("  lead and trail  "), 3)
        self.assertEqual(count_words("x\ty\r\nz  w"), 4)
        self.assertEqual(count_words("one"), 1)

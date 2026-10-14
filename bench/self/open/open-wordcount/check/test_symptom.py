import unittest

from textkit import count_words


class Symptom(unittest.TestCase):
    def test_double_space_and_newline(self):
        self.assertEqual(count_words("hello  world"), 2)
        self.assertEqual(count_words("a b\n"), 2)
        self.assertEqual(count_words("a\tb\nc"), 3)

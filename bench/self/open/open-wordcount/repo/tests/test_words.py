import unittest

from textkit import count_words, longest_word


class CountWords(unittest.TestCase):
    def test_plain_sentence(self):
        self.assertEqual(count_words("one two three"), 3)

    def test_empty(self):
        self.assertEqual(count_words(""), 0)

    def test_single_word(self):
        self.assertEqual(count_words("hello"), 1)


class Longest(unittest.TestCase):
    def test_first_of_equals(self):
        self.assertEqual(longest_word("aa bb c"), "aa")

    def test_none(self):
        self.assertEqual(longest_word(""), "")

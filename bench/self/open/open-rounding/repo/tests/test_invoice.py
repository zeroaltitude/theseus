import unittest

from money import invoice_total, round_half_up


class Rounding(unittest.TestCase):
    def test_plain(self):
        self.assertEqual(round_half_up(1.234), 1.23)
        self.assertEqual(round_half_up(1.236), 1.24)

    def test_negative_plain(self):
        self.assertEqual(round_half_up(-1.236), -1.24)


class Invoice(unittest.TestCase):
    def test_total(self):
        self.assertEqual(invoice_total([10.0, 5.0], 0.1), 16.5)

    def test_a_half_cent_rounds_up(self):
        self.assertEqual(round_half_up(0.125, 2), 0.13)

import unittest

from money import invoice_total, round_half_up


class Holdout(unittest.TestCase):
    def test_other_ties(self):
        self.assertEqual(round_half_up(2.5, 0), 3)
        self.assertEqual(round_half_up(0.375, 2), 0.38)
        self.assertEqual(round_half_up(1.005, 2), 1.01)
        self.assertEqual(round_half_up(-0.125, 2), -0.13)
        self.assertEqual(round_half_up(10.0, 2), 10.0)

    def test_totals(self):
        self.assertEqual(invoice_total([2.5], 0.05), 2.63)

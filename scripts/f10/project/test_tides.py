import unittest

import tides

TABLE = """
# Port Wenlow, an invented harbour
00:00 1.20
03:00 2.10
06:00 3.40
09:00 2.00
12:00 0.60
15:00 1.80
18:00 3.10
21:00 1.90
"""


class TidesTest(unittest.TestCase):
    def setUp(self):
        self.readings = tides.parse_table(TABLE)

    def test_parse_skips_comments(self):
        self.assertEqual(len(self.readings), 8)
        self.assertEqual(self.readings[0], tides.Reading("00:00", 1.2))

    def test_high_waters(self):
        self.assertEqual([r.time for r in tides.high_waters(self.readings)], ["06:00", "18:00"])

    def test_low_waters(self):
        self.assertEqual([r.time for r in tides.low_waters(self.readings)], ["12:00"])

    def test_tidal_range(self):
        self.assertEqual(tides.tidal_range(self.readings), 2.8)

    def test_format_height(self):
        self.assertEqual(tides.format_height(2.4), "2.40 m")


if __name__ == "__main__":
    unittest.main()

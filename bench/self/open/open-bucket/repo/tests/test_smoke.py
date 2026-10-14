import unittest

import ratelimit


class Smoke(unittest.TestCase):
    def test_imports(self):
        self.assertTrue(hasattr(ratelimit, "TokenBucket"))

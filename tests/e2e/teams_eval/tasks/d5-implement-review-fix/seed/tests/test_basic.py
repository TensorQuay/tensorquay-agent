import unittest
from limiter.bucket import RateLimiter


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


class Basic(unittest.TestCase):
    def test_burst_then_refused(self):
        clock = Clock()
        limiter = RateLimiter(1, 3, clock)
        self.assertEqual([limiter.allow("a") for _ in range(4)], [True, True, True, False])

    def test_refills(self):
        clock = Clock()
        limiter = RateLimiter(2, 2, clock)
        limiter.allow("a", 2)
        clock.now = 0.5
        self.assertTrue(limiter.allow("a"))

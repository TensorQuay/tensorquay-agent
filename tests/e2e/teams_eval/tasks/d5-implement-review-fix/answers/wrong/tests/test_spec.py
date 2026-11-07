import unittest
from limiter.bucket import RateLimiter


class Clock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now


class SpecVisible(unittest.TestCase):
    def test_keys_are_separate_and_start_full(self):
        clock = Clock()
        limiter = RateLimiter(1, 2, clock)
        self.assertTrue(limiter.allow("a", 2))
        self.assertTrue(limiter.allow("b", 2))
        self.assertAlmostEqual(limiter.remaining("c"), 2)

    def test_fractional_rate_and_the_cap(self):
        clock = Clock()
        limiter = RateLimiter(0.5, 4, clock)
        limiter.allow("a", 4)
        clock.now += 3
        self.assertAlmostEqual(limiter.remaining("a"), 1.5)
        clock.now += 1000
        self.assertAlmostEqual(limiter.remaining("a"), 4)

    def test_a_refusal_takes_nothing(self):
        clock = Clock()
        limiter = RateLimiter(1, 3, clock)
        limiter.allow("a", 2)
        self.assertFalse(limiter.allow("a", 2))
        self.assertAlmostEqual(limiter.remaining("a"), 1)

    def test_trap_cost_above_burst_is_never_allowed(self):
        clock = Clock()
        limiter = RateLimiter(10, 3, clock)
        self.assertFalse(limiter.allow("a", 4))
        self.assertAlmostEqual(limiter.remaining("a"), 3)

    def test_trap_cost_must_be_positive(self):
        limiter = RateLimiter(1, 3, Clock())
        for cost in (0, -1, "2", None):
            with self.assertRaises(ValueError):
                limiter.allow("a", cost)

    def test_trap_a_clock_set_back_adds_nothing(self):
        clock = Clock()
        limiter = RateLimiter(1, 5, clock)
        limiter.allow("a", 5)
        clock.now -= 50
        self.assertAlmostEqual(limiter.remaining("a"), 0)
        clock.now += 2
        self.assertAlmostEqual(limiter.remaining("a"), 2)

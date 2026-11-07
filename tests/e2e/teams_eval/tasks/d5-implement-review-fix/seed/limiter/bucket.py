"""A token-bucket rate limiter (spec/LIMITER.md)."""
import time


class RateLimiter:
    def __init__(self, rate, burst, clock=time.monotonic):
        raise NotImplementedError

    def allow(self, key, cost=1):
        raise NotImplementedError

    def remaining(self, key):
        raise NotImplementedError

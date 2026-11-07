"""A token-bucket rate limiter (spec/LIMITER.md)."""
import time


class RateLimiter:
    def __init__(self, rate, burst, clock=time.monotonic):
        self.rate, self.burst, self.clock = float(rate), float(burst), clock
        self.buckets = {}

    def _bucket(self, key):
        now = self.clock()
        tokens, seen = self.buckets.get(key, (self.burst, now))
        elapsed = now - seen
        tokens = min(self.burst, tokens + elapsed * self.rate)
        self.buckets[key] = (tokens, now)
        return tokens, now

    def allow(self, key, cost=1):
        if isinstance(cost, bool) or not isinstance(cost, (int, float)) or cost <= 0:
            raise ValueError("cost must be a positive number")
        tokens, now = self._bucket(key)
        if cost > self.burst or tokens < cost:
            return False
        self.buckets[key] = (tokens - cost, now)
        return True

    def remaining(self, key):
        return self._bucket(key)[0]

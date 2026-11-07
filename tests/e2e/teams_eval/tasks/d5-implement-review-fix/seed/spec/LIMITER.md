# Rate limiter

`limiter/bucket.py`'s `RateLimiter(rate, burst, clock=time.monotonic)` limits requests per key with a token bucket.

1. Each key has its own bucket; a new key's bucket starts full, with `burst` tokens.
2. A bucket gains `rate` tokens per second (a float, possibly below 1), continuously, up to `burst` at most.
3. `allow(key, cost=1)` takes `cost` tokens and returns True when the bucket holds at least `cost`; otherwise it
   takes nothing and returns False.
4. A request whose `cost` is larger than `burst` can never be allowed: it returns False and takes nothing, however
   full the bucket.
5. `cost` must be a positive number; anything else raises ValueError.
6. The clock may go backwards (a system clock set back): that interval adds no tokens, and the bucket goes on from
   the new reading.
7. `remaining(key)` returns the tokens the bucket holds now, without taking any.

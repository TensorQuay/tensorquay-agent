"""The agent-teams evaluation's D5 task (dependent, docs/evals/agent-teams.md section 2)."""


from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

# D5: implement to a spec, review against it, and fix what the review finds. Three of the spec's rules are easy to
# miss on a first pass (the traps), and only held-out tests over them pass.
D5_SPEC = """# Rate limiter

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
"""

D5_STUB = """\"\"\"A token-bucket rate limiter (spec/LIMITER.md).\"\"\"
import time


class RateLimiter:
    def __init__(self, rate, burst, clock=time.monotonic):
        raise NotImplementedError

    def allow(self, key, cost=1):
        raise NotImplementedError

    def remaining(self, key):
        raise NotImplementedError
"""

D5_SOLUTION = """\"\"\"A token-bucket rate limiter (spec/LIMITER.md).\"\"\"
import time


class RateLimiter:
    def __init__(self, rate, burst, clock=time.monotonic):
        self.rate, self.burst, self.clock = float(rate), float(burst), clock
        self.buckets = {}

    def _bucket(self, key):
        now = self.clock()
        tokens, seen = self.buckets.get(key, (self.burst, now))
        elapsed = max(0.0, now - seen)
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
"""

D5_TESTS = """import unittest
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
"""

D5_HIDDEN = """import unittest
from limiter.bucket import RateLimiter


class Clock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now


class Spec(unittest.TestCase):
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
"""


def d5():
    seed = {"spec/LIMITER.md": D5_SPEC, "limiter/__init__.py": "", "limiter/bucket.py": D5_STUB,
            "tests/__init__.py": "", "tests/test_basic.py": D5_TESTS,
            "README.md": f"# limiter\n\nA rate limiter to the spec in `spec/LIMITER.md`. Tests: `{TEST_COMMAND}`.\n"}
    review = ("# Review against spec/LIMITER.md\n\n| Rule | Verdict | Note |\n|---|---|---|\n"
              "| 1 separate keys, start full | ok | |\n| 2 refill up to burst | ok | capped with min() |\n"
              "| 3 a refusal takes nothing | ok | |\n"
              "| 4 cost above burst never allowed | fixed | first version allowed it on a full bucket |\n"
              "| 5 cost must be positive | fixed | first version took 0 and negatives |\n"
              "| 6 clock set back adds nothing | fixed | first version added negative time |\n"
              "| 7 remaining takes nothing | ok | |\n")
    solution = {"limiter/bucket.py": D5_SOLUTION, "REVIEW.md": review,
                "tests/test_spec.py": D5_HIDDEN.replace("class Spec", "class SpecVisible")}
    # A first pass that missed rule 6: a clock set back makes the elapsed time negative and drains the bucket.
    wrong = dict(solution)
    wrong["limiter/bucket.py"] = D5_SOLUTION.replace("elapsed = max(0.0, now - seen)", "elapsed = now - seen")
    check = """
from checklib import fail, has, held_out, read, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    review = read(workspace, "REVIEW.md")
    if not all(has(review, str(rule)) for rule in range(1, 8)):
        fail("REVIEW.md doesn't go through the spec's seven rules")
    return "the limiter meets every rule of the spec, its traps included, and the review covers each rule"

run(check)
"""
    return Task(
        id="d5-implement-review-fix", title="Implement a rate limiter to a spec, review it, fix what the review finds",
        kind="dependent",
        statement=f"""
Implement the rate limiter in `limiter/bucket.py` to the spec in `spec/LIMITER.md`. Then review the implementation
against the spec, rule by rule, write the review to `REVIEW.md` (each rule, its verdict, and what you changed), and fix
everything the review finds. Add tests to `tests/`; run them with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(80, 2100, 1_000_000), check_timeout=120,
        hidden={"test_hidden.py": D5_HIDDEN},
        key_facts=["Rule 4: a cost above burst is refused and takes nothing, even on a full bucket",
                   "Rule 5: a cost that isn't a positive number raises ValueError",
                   "Rule 6: a clock set back adds no tokens; the bucket goes on from the new reading",
                   "Refill is continuous at a possibly fractional rate, capped at burst",
                   "REVIEW.md goes through all seven rules with a verdict each"],
        solution=solution, wrong=wrong)

import unittest

from client import TransientError, fetch_with_retry


class Flaky:
    def __init__(self, failures, error=TransientError):
        self.failures, self.error, self.calls = failures, error, 0

    def __call__(self):
        self.calls += 1
        if self.calls <= self.failures:
            raise self.error(f"failure {self.calls}")
        return "ok"


class HeldOutTest(unittest.TestCase):
    def test_retries_with_exponential_backoff(self):
        delays, call = [], Flaky(2)
        self.assertEqual(fetch_with_retry(call, sleep=delays.append), "ok")
        self.assertEqual((call.calls, delays), (3, [0.5, 1.0]))

    def test_attempts_and_base_delay(self):
        delays, call = [], Flaky(4)
        self.assertEqual(fetch_with_retry(call, sleep=delays.append, attempts=5, base_delay=0.1), "ok")
        self.assertEqual(delays, [0.1, 0.2, 0.4, 0.8])

    def test_gives_up_with_the_last_error(self):
        delays, call = [], Flaky(9)
        with self.assertRaises(TransientError) as caught:
            fetch_with_retry(call, sleep=delays.append)
        self.assertEqual((call.calls, str(caught.exception), delays), (3, "failure 3", [0.5, 1.0]))

    def test_other_errors_are_not_retried(self):
        delays, call = [], Flaky(1, KeyError)
        with self.assertRaises(KeyError):
            fetch_with_retry(call, sleep=delays.append)
        self.assertEqual((call.calls, delays), (1, []))

    def test_success_needs_no_sleep(self):
        delays = []
        self.assertEqual(fetch_with_retry(lambda: 7, sleep=delays.append), 7)
        self.assertEqual(delays, [])

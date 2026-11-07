"""Calls a flaky service."""
import time


class TransientError(Exception):
    """A failure worth retrying, such as a timeout."""


def fetch_with_retry(call, sleep=time.sleep):
    """Return the result of `call()`."""
    return call()

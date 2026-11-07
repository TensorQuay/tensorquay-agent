"""An in-process job queue."""
import collections

JOBS = collections.deque()


def publish(kind, order_id):
    JOBS.append((kind, order_id))


def take():
    return JOBS.popleft() if JOBS else None

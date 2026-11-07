"""Writes event logs in the EVL1 format (spec/FORMAT.md)."""


def write_events(path, events):
    """Writes `events` (dicts with ts, level, service and message; level is a name such as "warn") to `path`."""
    raise NotImplementedError

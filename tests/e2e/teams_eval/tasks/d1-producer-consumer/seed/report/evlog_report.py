"""Reads EVL1 event logs (spec/FORMAT.md) and summarises them.
Run: python3 -m report.evlog_report FILE"""


def read_events(path):
    """The events in the file at `path`, in order, as dicts with ts, level (its name), service and message."""
    raise NotImplementedError


def summarize(path):
    """A summary of the file: total, by_level (name -> count, every level present), by_service (name -> count),
    errors (the messages of level error, in order), first_ts and last_ts (None for an empty log)."""
    raise NotImplementedError


if __name__ == "__main__":
    import json
    import sys

    print(json.dumps(summarize(sys.argv[1]), indent=1, ensure_ascii=False, sort_keys=True))

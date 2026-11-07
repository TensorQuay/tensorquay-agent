"""Reads EVL1 event logs (spec/FORMAT.md) and summarises them.
Run: python3 -m report.evlog_report FILE"""
import struct

LEVELS = ["debug", "info", "warn", "error"]


def read_events(path):
    """The events in the file at `path`, in order, as dicts with ts, level (its name), service and message."""
    data = open(path, "rb").read()
    if data[:5] != b"EVL1" + bytes([1]) or len(data) < 9:
        raise ValueError("not an EVL1 version 1 file")
    events, at, end = [], 5, len(data) - 4
    while at < end:
        ts, level = struct.unpack_from(">QB", data, at)
        at += 9
        texts = []
        for _ in range(2):
            (size,) = struct.unpack_from(">I", data, at)
            texts.append(data[at + 4:at + 4 + size].decode("utf-8"))
            at += 4 + size
        events.append({"ts": ts, "level": LEVELS[level], "service": texts[0], "message": texts[1]})
    (count,) = struct.unpack_from(">I", data, end)
    if at != end or count != len(events):
        raise ValueError("the trailer doesn't match the records")
    return events


def summarize(path):
    """A summary of the file: total, by_level (name -> count, every level present), by_service (name -> count),
    errors (the messages of level error, in order), first_ts and last_ts (None for an empty log)."""
    events = read_events(path)
    by_service = {}
    for event in events:
        by_service[event["service"]] = by_service.get(event["service"], 0) + 1
    return {"total": len(events),
            "by_level": {level: sum(1 for event in events if event["level"] == level) for level in LEVELS},
            "by_service": by_service,
            "errors": [event["message"] for event in events if event["level"] == "error"],
            "first_ts": events[0]["ts"] if events else None, "last_ts": events[-1]["ts"] if events else None}


if __name__ == "__main__":
    import json
    import sys

    print(json.dumps(summarize(sys.argv[1]), indent=1, ensure_ascii=False, sort_keys=True))

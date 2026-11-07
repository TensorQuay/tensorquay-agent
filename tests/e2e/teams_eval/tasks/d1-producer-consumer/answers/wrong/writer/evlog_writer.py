"""Writes event logs in the EVL1 format (spec/FORMAT.md)."""
import struct

LEVELS = ["debug", "info", "warn", "error"]


def _text(value):
    data = value.encode("utf-8")
    return struct.pack(">H", len(data)) + data


def write_events(path, events):
    """Writes `events` (dicts with ts, level, service and message; level is a name such as "warn") to `path`."""
    with open(path, "wb") as file:
        file.write(b"EVL1" + bytes([1]))
        for event in events:
            file.write(struct.pack(">QB", event["ts"], LEVELS.index(event["level"])))
            file.write(_text(event["service"]) + _text(event["message"]))
        file.write(struct.pack(">I", len(events)))

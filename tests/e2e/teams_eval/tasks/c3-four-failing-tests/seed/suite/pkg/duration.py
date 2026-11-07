"""Durations."""
import re

UNITS = {"h": 3600, "m": 60, "s": 1}


def parse_duration(text):
    """'1h30m', '45m', '2h' or '90s' in seconds."""
    match = re.match(r"(\d+)([hms])", text)
    if match is None:
        raise ValueError(f"not a duration: {text!r}")
    return int(match.group(1)) * UNITS[match.group(2)]

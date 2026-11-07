"""Durations."""
import re

UNITS = {"h": 3600, "m": 60, "s": 1}


def parse_duration(text):
    """'1h30m', '45m', '2h' or '90s' in seconds."""
    total = 0
    for amount, unit in re.findall(r"(\d+)([hms])", text):
        total += int(amount) * UNITS[unit]
    if not total and not re.fullmatch(r"(\d+[hms])+", text):
        raise ValueError(f"not a duration: {text!r}")
    return total

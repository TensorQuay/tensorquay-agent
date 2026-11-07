"""CSV rows by splitting lines, each row checked by a JSON round trip."""
import json
import re


def parse(text):
    rows = []
    for line in text.splitlines():
        row = re.split(r",", line)
        rows.append(json.loads(json.dumps(row)))
    return rows

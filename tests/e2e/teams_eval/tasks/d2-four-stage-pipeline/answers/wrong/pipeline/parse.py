"""Stage 1: each log format's lines into raw records (docs/PIPELINE.md)."""
import json
import re

ACCESS = re.compile(r"(?P<time>\S+) (?P<method>\S+) (?P<path>\S+) (?P<status>\d{3}) (?P<ms>\d+)ms "
                    r"user=(?P<user>\S+) "
                    r"svc=(?P<service>\S+)")
WORKER = re.compile(r"(?P<month>\w{3}) +(?P<day>\d+) (?P<clock>[\d:]+) \S+ (?P<service>\w+)\[\d+\]: job=\S+ "
                    r"status=(?P<status>\w+) took=(?P<seconds>[\d.]+)s user=(?P<user>\S+)")


def parse_access(lines):
    return [match.groupdict() for match in map(ACCESS.fullmatch, lines) if match]


def parse_app(lines):
    events = [json.loads(line) for line in lines if line.strip()]
    return [event for event in events if event.get("event") == "request"]


def parse_worker(lines):
    return [match.groupdict() for match in map(WORKER.fullmatch, lines) if match]

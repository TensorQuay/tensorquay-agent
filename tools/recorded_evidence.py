"""Recorded gate evidence: a result file that stands in for running its ID (`--recorded ID=PATH[@SHA256]`).
- SOAK-001…004: the soak's result.json: status=passed, gate_evidence=true, and chaos_seconds of at least 2 h for
  Gate 2 or 24 h for any other gate.
- Any other ID: a TEST-PLAN §15.1 result (the PERF harnesses' files): its id is the ID or one part of it (PERF-011's
  `PERF-011-new-task`), its environment is there and was measured on a clean tree, and its verdict is pass.
Every file is labelled with its SHA-256; a SHA-256 given after `@` must match it.
"""
import hashlib
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path

MAX_BYTES = 4 * 1024 * 1024
ENVIRONMENT = ("utc", "kernel", "cpu", "commit", "dirty_tree")
ENTRY = re.compile(r"(?P<id>[A-Z0-9]+(?:-[A-Z0-9]+)+)=(?P<path>.+?)(?:@(?P<sha256>[0-9a-f]{64}))?")


@dataclass(frozen=True)
class RecordedEvidence:
    path: Path
    sha256: str = None


def parse(entry):
    """`ID=PATH` or `ID=PATH@SHA256` as (ID, RecordedEvidence)."""
    match = ENTRY.fullmatch(entry)
    if match is None or "@" in match["path"]:
        raise ValueError("--recorded takes ID=path/to/result.json, optionally @ its SHA-256: " + json.dumps(entry))
    return match["id"], RecordedEvidence(Path(match["path"]), match["sha256"])


def unique_fields(pairs):
    fields = {}
    for key, value in pairs:
        if key in fields:
            raise ValueError("duplicate field in recorded evidence")
        fields[key] = value
    return fields


def read(artifact):
    """The file's bytes and parsed JSON, refused when missing, too large, malformed or not the pinned file."""
    if not artifact.path.is_file():
        raise ValueError("recorded evidence must be an existing regular file")
    with artifact.path.open("rb") as source:
        data = source.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("recorded evidence exceeds 4 MiB")
    digest = hashlib.sha256(data).hexdigest()
    if artifact.sha256 is not None and artifact.sha256 != digest:
        raise ValueError("its sha256 differs from the one given: {} is {}".format(artifact.sha256, digest))
    return digest, json.loads(data, object_pairs_hook=unique_fields)


def soak(result, gate):
    if not isinstance(result, dict) or result.get("status") != "passed" or result.get("gate_evidence") is not True:
        raise ValueError("recorded soak evidence requires status=passed and gate_evidence=true")
    seconds, minimum = result.get("chaos_seconds"), 7200 if gate == "2" else 86400
    if type(seconds) not in (int, float) or not math.isfinite(seconds) or seconds < minimum:
        raise ValueError("recorded soak evidence requires chaos_seconds >= " + str(minimum))
    return "status=passed gate_evidence=true chaos_seconds={}".format(seconds)


def measured(identifier, result):
    if not isinstance(result, dict):
        raise ValueError("it is not a result object")
    named = result.get("id")
    if not isinstance(named, str) or not (named == identifier or named.startswith(identifier + "-")):
        raise ValueError("it is {}, not {}".format(named, identifier))
    environment = result.get("environment")
    if not isinstance(environment, dict):
        raise ValueError("it has no environment: a result without its environment is not a result")
    missing = [field for field in ENVIRONMENT if field not in environment]
    if missing:
        raise ValueError("its environment lacks " + ", ".join(missing))
    if environment["dirty_tree"] is not False:
        raise ValueError("it was measured on a dirty tree")
    if result.get("verdict") != "pass":
        raise ValueError("its verdict is {}, not pass".format(result.get("verdict")))
    return "id={} verdict=pass commit={}".format(named, str(environment["commit"])[:12])


def describe(identifier, artifact, gate):
    """The evidence line for a valid recorded file; ValueError says why one isn't."""
    digest, result = read(artifact)
    detail = soak(result, gate) if identifier.startswith("SOAK-") else measured(identifier, result)
    return "recorded {} sha256={} {}".format(json.dumps(str(artifact.path)), digest, detail)

"""The Gate 9 measurement run's rules (DEV-PLAN §4b item 3), kept apart from running anything so they are unit tested:
- the steps and their jobs: a (PERF-001/002/003, built from source), b (PERF-010, PERF-020), c (PERF-021), d (PERF-011,
  KVM, only with --kvm), e (the 24 h soak at 8 conversations);
- §15.1 item 3: PERF-001/002/003/010/011/020 are compared with the previous release in ABBA order (baseline,
  candidate, candidate, baseline) on the same host in one run, so drift during the run shows instead of biasing one
  side;
- a job starts only on a quiet host (one-minute load under 10) and is disturbed if the load rose above 10 during it:
  a disturbed result is kept but never recorded as gate evidence;
- what gate-evidence may take as recorded evidence: an ID whose every candidate run passed undisturbed.
"""
import re
import statistics

from soak_audit import AuditError, require

__all__ = ["AuditError"]

LOAD_LIMIT = 10.0
REGRESSION = 0.10
ORDER = ("a", "b", "c", "d", "e")
STEPS = {"a": ("PERF-001", "PERF-002", "PERF-003"), "b": ("PERF-010", "PERF-020"), "c": ("PERF-021",),
         "d": ("PERF-011",), "e": ("SOAK",)}
COMPARED = {"PERF-001", "PERF-002", "PERF-003", "PERF-010", "PERF-011", "PERF-020"}
ABBA = ("baseline", "candidate", "candidate", "baseline")
# A gateway that takes pinned built-ins needs the Workstation image too, so a stack step checks for it before it runs.
STACK_IMAGES = ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_RUNTIME_IMAGE", "TQA_FAKE_MODEL_IMAGE",
                "TQA_WORKSTATION_IMAGE")
IMAGES = {"a": (), "b": STACK_IMAGES, "c": STACK_IMAGES, "d": STACK_IMAGES + ("TQA_VMM_IMAGE",),
          "e": ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_WORKSTATION_IMAGE", "TQA_FAKE_MODEL_IMAGE")}
SOAK_IDS = ("SOAK-001", "SOAK-002", "SOAK-003", "SOAK-004")
# Where each compared figure is in its result, and the change under which a difference is noise whatever its percent.
FIGURES = {
    "PERF-001": {"journal_p50_ms": ("result", "variants", "journal", "p50_ms"),
                 "journal_p99_ms": ("result", "variants", "journal", "p99_ms")},
    "PERF-002": {"lag_p50_ms": ("result", "lag_ms", "p50"), "lag_p99_ms": ("result", "lag_ms", "p99")},
    "PERF-003": {"added_total_p50_ms": ("result", "added_total_ms", "p50"),
                 "added_first_token_p50_ms": ("result", "added_first_token_ms", "p50")},
    "PERF-010": {"p50_ms": ("summary", "p50_ms"), "p99_ms": ("summary", "p99_ms")},
    "PERF-011": {"p50_ms": ("summary", "p50_ms"), "p99_ms": ("summary", "p99_ms")},
    "PERF-020": {"cpu_p95_percent": ("summary", "cpu_p95_percent"), "rss_peak_bytes": ("summary", "rss_peak_bytes")},
}
FLOORS = {"PERF-001": 0.05, "PERF-002": 0.5, "PERF-003": 1.0, "PERF-010": 50.0, "PERF-011": 50.0,
          "cpu_p95_percent": 2.0, "rss_peak_bytes": 10_000_000}
# Results each run writes: PERF-010 a fresh-sandbox and a returning-task one; PERF-011 the first start, a new task, a
# returning task and a new task's upload
PARTS = {"PERF-010": 2, "PERF-011": 4}
TAG = re.compile(r"(TQA_[A-Z_]+_IMAGE)=(sha256:[0-9a-f]{64})")


class Job:
    """One harness run: its step, the ID it measures, which side (baseline or candidate) and its place in the order."""
    def __init__(self, step, identifier, side, round_):
        self.step, self.identifier, self.side, self.round = step, identifier, side, round_

    def __repr__(self):
        return "Job({}, {}, {}, {})".format(self.step, self.identifier, self.side, self.round)


def step_of(identifier):
    """The step that runs an ID; an ID no step runs is refused, naming the ones that exist."""
    step = next((step for step in ORDER if identifier in STEPS[step]), None)
    require(step is not None, "unknown ID {}: the IDs are {}".format(identifier, ", ".join(
        known for step in ORDER for known in STEPS[step])))
    return step


def jobs(steps, kvm=False, ids=None, candidate_only=False):
    """Every job of the chosen steps, or of the chosen IDs alone, in the fixed step order; compared IDs four times in
    ABBA order, or once on the candidate in a candidate-only run (a reproduction, never gate evidence)."""
    for step in steps:
        require(step in STEPS, "unknown step {}: the steps are {}".format(step, ", ".join(ORDER)))
    whole, chosen = set(steps), set(ids or ())
    steps = whole | {step_of(identifier) for identifier in chosen}
    require("d" not in steps or kvm, "step d needs KVM: pass --kvm once the KVM run is agreed")
    planned = []
    for step in (step for step in ORDER if step in steps):
        for identifier in (identifier for identifier in STEPS[step] if step in whole or identifier in chosen):
            sides = ABBA if identifier in COMPARED and not candidate_only else ("candidate",)
            planned += [Job(step, identifier, side, number) for number, side in enumerate(sides, 1)]
    return planned


def parse_tags(text):
    """A candidate's tag file: `TQA_*_IMAGE=sha256:<id>` lines; comments and blank lines are skipped."""
    tags = {}
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        match = TAG.fullmatch(line)
        require(match is not None, "tag file line {} is not TQA_*_IMAGE=sha256:<image ID>".format(number))
        require(match[1] not in tags, "tag file line {} names {} again".format(number, match[1]))
        tags[match[1]] = match[2]
    return tags


def check_tags(tags, steps, side):
    """Each side's tag file holds the images its steps run; the baseline never runs the soak."""
    for step in (step for step in ORDER if step in steps):
        if side == "baseline" and not any(identifier in COMPARED for identifier in STEPS[step]):
            continue
        for name in IMAGES[step]:
            require(name in tags, "the {} tag file lacks {} (step {})".format(side, name, step))


def quiet(loadavg):
    """Whether /proc/loadavg's one-minute load is under the limit."""
    return float(loadavg.split()[0]) < LOAD_LIMIT


def disturbed(samples):
    """(disturbed, highest one-minute load) over a run's load samples."""
    require(bool(samples), "no load samples for the run")
    return max(samples) > LOAD_LIMIT, max(samples)


def base_id(identifier):
    """PERF-011-new-task is a part of PERF-011."""
    return next((known for known in FIGURES if identifier == known or identifier.startswith(known + "-")), identifier)


def metrics(result):
    """The compared figures of one result file, by name."""
    identifier = result.get("id", "")
    figures = {}
    for name, path in FIGURES[base_id(identifier)].items():
        value = result
        for key in path:
            value = value.get(key) if isinstance(value, dict) else None
        require(isinstance(value, (int, float)), "{} result lacks {}".format(identifier, ".".join(path)))
        figures[name] = value
    return figures


def _by_name(identifier, results):
    """Each run's figures; PERF-011's parts are named by part (`new-task.p50_ms`)."""
    named = {}
    for result in results:
        part = result["id"][len(identifier) + 1:]
        for name, value in metrics(result).items():
            named.setdefault(part + "." + name if part else name, []).append(value)
    return named


def compare(identifier, baseline, candidate, excluded=()):
    """ABBA: each figure's mean on each side; a regression is over 10 % worse and over the figure's noise floor.
    `excluded` names the baseline runs left out (failed or disturbed): the rest compare, indicatively."""
    per_run = PARTS.get(identifier, 1)
    counts = len(baseline) // per_run, len(candidate) // per_run
    if excluded and counts[0] == 0:
        return {"compared": False, "reason": "baseline not measurable: " + "; ".join(excluded)}
    if counts != (2 - min(len(excluded), 1), 2) and counts != (2, 2):
        return {"compared": False, "reason": "{} baseline and {} candidate results; ABBA needs 2 of each".format(
            *counts)}
    before, after = _by_name(identifier, baseline), _by_name(identifier, candidate)
    figures = {}
    for name, values in before.items():
        old, new = statistics.mean(values), statistics.mean(after[name])
        floor = FLOORS.get(name.split(".")[-1])
        if floor is None:
            require(identifier in FLOORS, "no noise floor for {} {}".format(identifier, name))
            floor = FLOORS[identifier]
        figure = {}
        if old <= 0 and name.split(".")[-1].startswith("added_"):
            # An added latency at or under zero isn't comparable as a percentage (Gate 9 on c14b-rc: 13h's PERF-003
            # total added p50 was -31.9 and -30.6 ms): the baseline counts as 0, the candidate by the floor alone.
            figure["note"] = ("baseline added is {:g} ms, at or under 0: not comparable as a percentage; clamped to 0 "
                              "and judged by the absolute floor".format(old))
            old = 0.0
        relative = (new - old) / old if old > 0 else None
        figures[name] = dict({"baseline": old, "candidate": new, "change": relative,
                              "baseline_drift": abs(values[0] - values[-1]) / old if old > 0 else None,
                              "regression": new - old > floor and (relative is None or relative > REGRESSION)},
                             **figure)
    return figures


def soak_verdict(result):
    """A soak counts only as passed gate evidence (never a rehearsal)."""
    return "pass" if result.get("status") == "passed" and result.get("gate_evidence") is True else "fail"


REPRODUCTION = "a candidate-only run reproduces the number; the gate takes it from the ABBA run"


def recorded(job_records, candidate_only=False):
    """`--recorded` entries for gate-evidence, pinned by SHA-256, for each ID whose every candidate run passed
    undisturbed; the reason for each ID that isn't. A compared ID from a candidate-only run is never recorded."""
    entries, reasons, by_id = [], {}, {}
    for record in job_records:
        if record["side"] == "candidate":
            by_id.setdefault(record["id"], []).append(record)
    for identifier, records in by_id.items():
        reason = None
        for record in records:
            if record["disturbed"] is None:
                reason = "a candidate run's load record was lost, so it can't be shown undisturbed"
            elif record["disturbed"]:
                reason = "a candidate run was disturbed (load over {:.0f})".format(LOAD_LIMIT)
            elif not record["results"]:
                reason = "a candidate run exited {} without a result".format(record["exit"])
            elif any(result["verdict"] != "pass" for result in record["results"]):
                reason = "a candidate result's verdict is fail"
            if reason:
                break
        if not reason and candidate_only and identifier in COMPARED:
            reason = REPRODUCTION
        if reason:
            reasons[identifier] = reason
            continue
        first = records[0]["results"][0]
        for name in (SOAK_IDS if identifier == "SOAK" else (identifier,)):
            entries.append("{}={}@{}".format(name, first["path"], first["sha256"]))
    return entries, reasons


# The Rust measurements: package, test binary and the one ignored test.
RUST = {"PERF-001": ("tqa-core", "performance", "perf_001_harness_overhead"),
        "PERF-002": ("tqa-gateway", "perf_event_pipeline", "perf_002_event_pipeline_throughput"),
        "PERF-003": ("tqa-gateway", "perf_relay", "perf_003_relay_latency")}
# The stack measurements and the soak: their harness, always the candidate's (a baseline run takes baseline images).
PYTHON = {"PERF-010": ["deploy/tests/perf_start.py"],
          "PERF-011": ["deploy/tests/perf_start.py", "--backend", "firecracker"],
          "PERF-020": ["deploy/tests/perf_concurrency.py"], "PERF-021": ["deploy/tests/perf_idle.py"],
          "SOAK": ["deploy/tests/soak.py", "--hours", "24", "--conversations", "8", "--seed", "731"]}
# A Rust harness is compiled into the code it measures, so the baseline tree gets the candidate's harness files.
HARNESS = {"PERF-001": ["crates/tqa-core/tests/performance.rs", "crates/tqa-core/tests/performance"],
           "PERF-002": ["crates/tqa-gateway/tests/perf_event_pipeline.rs",
                        "crates/tqa-gateway/tests/perf_event_pipeline", "crates/tqa-gateway/tests/support/runtime.rs"],
           "PERF-003": ["crates/tqa-gateway/tests/perf_relay.rs", "crates/tqa-gateway/tests/perf_relay"]}
TESTKIT = ["crates/tqa-testkit/src/perf.rs", "crates/tqa-testkit/src/perf_tests.rs"]
TESTKIT_LIB = "crates/tqa-testkit/src/lib.rs"
TESTKIT_MANIFEST = "crates/tqa-testkit/Cargo.toml"


def rust_command(identifier, build):
    package, binary, name = RUST[identifier]
    command = ["cargo", "nextest", "run", "--release", "-p", package, "--test", binary, "--run-ignored", "all"]
    return command + (["--no-run"] if build else ["--no-capture", "-E", "test(={})".format(name)])


def python_command(identifier, output):
    return ["python3", "-B"] + PYTHON[identifier] + ([] if output is None else ["--output", str(output)])


def overlay(identifiers):
    """The candidate's harness files a baseline tree needs for these IDs (none for the stack harnesses)."""
    paths = [path for identifier in identifiers for path in HARNESS.get(identifier, ())]
    return paths + TESTKIT if paths else []


def with_dependencies(baseline, candidate):
    """The baseline testkit's manifest with each `[dependencies]` line of the candidate's that it lacks, by crate name,
    added at the end of its own `[dependencies]`: the candidate's perf kit is laid over the baseline and must build
    there (its `sha2`, which 13h's testkit lacks). The baseline's own versions and every other table are kept."""
    def section(text):
        names, inside, end = {}, False, None
        lines = text.split("\n")
        for number, line in enumerate(lines):
            stripped = line.strip()
            if stripped.startswith("["):
                if inside:
                    break
                inside = stripped == "[dependencies]"
                continue
            if inside and "=" in stripped and not stripped.startswith("#"):
                names[stripped.split("=", 1)[0].strip()] = line
                end = number
        return lines, names, end
    lines, have, end = section(baseline)
    _, wanted, _ = section(candidate)
    missing = [line for name, line in wanted.items() if name not in have]
    require(end is not None or not missing, "the baseline testkit's manifest has no [dependencies] to add to")
    if not missing:
        return baseline
    return "\n".join(lines[:end + 1] + missing + lines[end + 1:])


def with_perf_module(text):
    """The testkit's lib.rs with its perf module declared, once."""
    return text if re.search(r"^pub mod perf;$", text, flags=re.MULTILINE) else text + "pub mod perf;\n"


def new_files(before, after):
    """The result files a job added: not its log directories, nor the raw data beside a result (`<result>.<name>.json`,
    which the result names)."""
    return sorted(name for name in after - before if name.endswith(".json") and name.count("/") <= 1
                  and name.rsplit("/", 1)[-1].count(".") == 1)


def raw_files(result):
    """The raw data files a result names beside itself, each with its SHA-256 (none for most results)."""
    named = (result.get("result") or {}).get("raw_files", []) if isinstance(result.get("result"), dict) else []
    for entry in named:
        require(isinstance(entry, dict) and "/" not in str(entry.get("file", "/")) and entry.get("sha256"),
                "{} names a raw file without its plain name and SHA-256".format(result.get("id")))
    return named

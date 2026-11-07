"""Gate 9's measurements on one candidate (DEV-PLAN §4b item 3; PRD M8's reproduction script). Runs each step's existing
harnesses unchanged, compares PERF-001/002/003/010/011/020 with the previous release in ABBA order, and writes an
index of every result with its SHA-256, the comparisons, and the `--recorded` entries gate-evidence takes.
From a clean checkout of the candidate commit, one step at a time or several (see gate9_plan.py for the rules):
    TQA_DOCKER_TESTS=1 python3 deploy/tests/gate9_run.py --candidate-tags C --baseline-tags B \\
        --baseline-commit <previous release> --steps a,b,c [--kvm]
One published number alone, on the candidate only (a reproduction, never gate evidence), and what a run would do:
    TQA_DOCKER_TESTS=1 python3 deploy/tests/gate9_run.py --candidate-tags C --ids PERF-010 --candidate-only
    python3 deploy/tests/gate9_run.py --candidate-tags C --ids PERF-002,PERF-003 --candidate-only --dry-run
Step a needs DATABASE_URL and TQA_DATABASE_URL (the same PostgreSQL); the Rust builds use CARGO_TARGET_DIR/<side>.
The host must be otherwise idle: each job waits up to --quiet-wait seconds for a one-minute load under 10.
"""
import re
import argparse
import datetime
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import gate9_plan as plan
from soak_audit import AuditError, require

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "tests" / "perf" / "results"
LOAD_EVERY = 5
SOAK_SECONDS = 26 * 3600
JOB_SECONDS = 3 * 3600
TEARDOWN_SECONDS = 150  # after SIGTERM, for a harness to remove its stack


def git(*arguments, cwd=ROOT):
    return subprocess.run(["git"] + list(arguments), cwd=cwd, capture_output=True, text=True, check=True).stdout.strip()


def loadavg():
    return Path("/proc/loadavg").read_text()


def checkout(commit, directory):
    """A detached worktree at `commit`, so a branch switch in this checkout can't change a run under way, and each
    result's environment still names its commit and whether its tree was clean."""
    git("worktree", "add", "--detach", str(directory), commit)


def bounded(command, cwd, env, seconds):
    """Runs a harness; past `seconds` it gets SIGTERM (it tears its stack down on it), then SIGKILL."""
    process = subprocess.Popen(command, cwd=cwd, env=env)
    try:
        return process.wait(seconds)
    except subprocess.TimeoutExpired:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(TEARDOWN_SECONDS)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        return "timeout after {} s".format(seconds)


def lay_over(baseline, candidate, identifiers):
    """The candidate's harness files over the baseline's code, for the Rust measurements."""
    for relative in plan.overlay(identifiers):
        source, target = candidate / relative, baseline / relative
        if target.is_dir():
            shutil.rmtree(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        (shutil.copytree if source.is_dir() else shutil.copy2)(source, target)
    lib = baseline / plan.TESTKIT_LIB
    lib.write_text(plan.with_perf_module(lib.read_text()))
    manifest = baseline / plan.TESTKIT_MANIFEST
    if manifest.is_file():
        manifest.write_text(plan.with_dependencies(manifest.read_text(),
                                                   (candidate / plan.TESTKIT_MANIFEST).read_text()))


def keep_raw(path, kept, data):
    """Copies the raw data a result names from beside it to beside its kept copy; a file missing or changed since the
    result named it fails the result."""
    copied = []
    for entry in plan.raw_files(data):
        source = path.parent / entry["file"]
        target = kept.parent / (kept.name[:-len(path.name)] + entry["file"])
        if source.is_file():
            shutil.copy2(source, target)
        sha256 = hashlib.sha256(target.read_bytes()).hexdigest() if target.is_file() else None
        copied.append({"path": str(target.relative_to(ROOT)), "sha256": sha256, "matches": sha256 == entry["sha256"]})
    return copied


def snapshot(results):
    return {str(path.relative_to(results)) for path in results.rglob("*.json")} if results.is_dir() else set()


class LoadWatch(threading.Thread):
    """Samples the one-minute load every few seconds while a job runs."""
    def __init__(self):
        super().__init__(daemon=True)
        self.samples, self.stopping = [], threading.Event()

    def run(self):
        while True:
            self.samples.append(float(loadavg().split()[0]))
            if self.stopping.wait(LOAD_EVERY):
                return


def wait_quiet(seconds):
    deadline = time.monotonic() + seconds
    while not plan.quiet(loadavg()):
        require(time.monotonic() < deadline, "the host was not quiet (load {}) within {} s".format(
            loadavg().split()[0], seconds))
        time.sleep(30)


class Runner:
    def __init__(self, options, work):
        self.options, self.work, self.records = options, work, []
        self.trees = {"candidate": work / "candidate"}
        self.tags = {"candidate": plan.parse_tags(Path(options.candidate_tags).read_text())}
        if options.baseline_tags:
            self.tags["baseline"] = plan.parse_tags(Path(options.baseline_tags).read_text())
        stamp = datetime.datetime.now(datetime.timezone.utc)
        self.stamp = stamp.strftime("%Y-%m-%dT%H-%M-%SZ")
        self.keep = RESULTS / stamp.strftime("%Y-%m-%d") / ("gate9-" + self.stamp)

    def check(self, jobs):
        """Everything a run needs, checked without creating anything: a clean tree, each side's images, the
        previous release's commit and the database a Rust job needs."""
        require(not git("status", "--porcelain"), "the checkout has uncommitted changes: a result needs a clean tree")
        steps = {job.step for job in jobs}
        compared = any(job.side == "baseline" for job in jobs)
        plan.check_tags(self.tags["candidate"], steps, "candidate")
        if compared:
            require("baseline" in self.tags and self.options.baseline_commit,
                    "a compared measurement needs --baseline-tags and --baseline-commit (the previous release)")
            plan.check_tags(self.tags["baseline"], steps, "baseline")
            git("rev-parse", "--verify", self.options.baseline_commit + "^{commit}")
        if any(job.identifier in plan.RUST for job in jobs):
            require(os.environ.get("DATABASE_URL") and os.environ.get("DATABASE_URL") == os.environ.get(
                "TQA_DATABASE_URL"), "step a needs DATABASE_URL and TQA_DATABASE_URL set to the same database")
        images = {image for side, tags in self.tags.items() for step in steps for name, image in tags.items()
                  if name in plan.IMAGES[step]}
        for image in sorted(images):
            subprocess.run(["docker", "image", "inspect", image], capture_output=True, check=True, timeout=60)

    def prepare(self, jobs):
        """The candidate's detached tree, and the baseline's with the candidate's Rust harness laid over it."""
        checkout("HEAD", self.trees["candidate"])
        baseline = sorted({job.identifier for job in jobs if job.side == "baseline"})
        if baseline:
            self.trees["baseline"] = self.work / "baseline"
            checkout(self.options.baseline_commit, self.trees["baseline"])
            lay_over(self.trees["baseline"], self.trees["candidate"], baseline)

    def describe(self, jobs):
        """A dry run's account of each job: its command, the tree it runs in, its build directory and images."""
        lines = ["host quiet rule: each job waits up to {} s for a one-minute load under {:.0f}; a result is "
                 "disturbed if it rose above {:.0f} during the job".format(self.options.quiet_wait, plan.LOAD_LIMIT,
                                                                          plan.LOAD_LIMIT)]
        for job in jobs:
            rust = job.identifier in plan.RUST
            side = job.side if rust else "candidate"
            command = plan.rust_command(job.identifier, build=False) if rust else plan.python_command(
                job.identifier, self.work / "soak" if job.identifier == "SOAK" else None)
            env = self.environment(job)
            tree = "the candidate (HEAD)" if side == "candidate" else "the baseline ({}) with the candidate's " \
                "harness".format(self.options.baseline_commit)
            images = ", ".join("{}={}".format(name, env[name][:19]) for name in plan.IMAGES[job.step])
            lines.append("{} {} round {}: {} | in {}{}{}".format(
                job.identifier, job.side, job.round, " ".join(command), tree,
                " | CARGO_TARGET_DIR " + env["CARGO_TARGET_DIR"] if rust else "",
                " | images " + images if images else ""))
        return lines

    def environment(self, job):
        env = dict(os.environ, SQLX_OFFLINE="true",
                   CARGO_TARGET_DIR=str(Path(os.environ.get("CARGO_TARGET_DIR", self.work / "target")) / job.side))
        env.update(self.tags[job.side])
        if job.side == "baseline":
            # The previous release's gateway predates [runtime.builtin_images]: its stack takes the engine's tags,
            # says so, and the sandbox image guard still fails a sandbox on an image not under test.
            env["TQA_UNPINNED_BUILTINS"] = "1"
        if job.identifier == "SOAK":
            env.update(TQA_RUNTIME_IMAGE=env["TQA_WORKSTATION_IMAGE"], TQA_ALLOW_DEV_HOST_GATEWAY="1")
        return env

    def run(self, job):
        """Builds (a Rust job), waits for quiet, runs under the load watch, and keeps every result file it wrote."""
        rust = job.identifier in plan.RUST
        tree = self.trees[job.side] if rust else self.trees["candidate"]
        env, output = self.environment(job), self.work / "soak" if job.identifier == "SOAK" else None
        record = {"id": job.identifier, "side": job.side, "round": job.round, "step": job.step, "results": []}
        print("== {} {} round {} ({})".format(job.identifier, job.side, job.round, time.strftime("%H:%M")), flush=True)
        if rust:
            built = subprocess.run(plan.rust_command(job.identifier, build=True), cwd=tree, env=env)
            if built.returncode:
                record.update(exit=built.returncode, disturbed=False,
                              reason="the harness does not build on the {} tree".format(job.side))
                return record
        try:
            wait_quiet(self.options.quiet_wait)
        except AuditError as error:
            record.update(exit="not started", disturbed=False, reason=str(error))
            return record
        results = tree / "tests" / "perf" / "results"
        before, watch = snapshot(results), LoadWatch()
        watch.start()
        command = plan.rust_command(job.identifier, build=False) if rust else plan.python_command(job.identifier,
                                                                                                    output)
        exit_code = bounded(command, tree, env, SOAK_SECONDS if output else JOB_SECONDS)
        watch.stopping.set()
        watch.join()
        record["disturbed"], record["max_load"] = plan.disturbed(watch.samples)
        record["exit"] = exit_code
        files = [results / name for name in plan.new_files(before, snapshot(results))]
        if output is not None and (output / "result.json").is_file():
            files = [output / "result.json"]
        for path in files:
            kept = self.keep / "{}-{}-{}-{}".format(job.identifier, job.round, job.side, path.name)
            kept.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, kept)
            data = json.loads(kept.read_text())
            verdict = plan.soak_verdict(data) if output is not None else data.get("verdict", "fail")
            record["results"].append({"path": str(kept.relative_to(ROOT)), "sha256": hashlib.sha256(
                kept.read_bytes()).hexdigest(), "id": data.get("id", "SOAK"), "verdict": verdict,
                                      "raw": keep_raw(path, kept, data)})
            if any(not raw["matches"] for raw in record["results"][-1]["raw"]):
                record["results"][-1]["verdict"] = "fail"
        return record

    def keep_record(self, record):
        """Appends a finished job's record beside its results, so a later failure never loses its load (the c14b run,
        5 Oct 2026, lost every record when its index failed)."""
        self.keep.mkdir(parents=True, exist_ok=True)
        with (self.keep / "jobs.jsonl").open("a") as jobs:
            jobs.write(json.dumps(record) + "\n")

    def comparisons(self):
        loaded, excluded = {}, {}
        for record in self.records:
            if record["side"] == "baseline" and (record.get("disturbed") or any(
                    result["verdict"] != "pass" for result in record["results"]) or not record["results"]):
                # A failed or disturbed baseline run isn't the previous release's number (the c14b retake, 5 Oct 2026).
                why = ("disturbed, load up to {}".format(record.get("max_load")) if record.get("disturbed")
                       else "failed" if record["results"] else "no result")
                excluded.setdefault(record["id"], []).append("round {} {}".format(record["round"], why))
                continue
            for result in record["results"]:
                loaded.setdefault((record["id"], record["side"]), []).append(json.loads((ROOT / result["path"])
                                                                                        .read_text()))
                # (ROOT / an absolute path) is that path: --index-only's records name their kept files absolutely.
        compared = {}
        for identifier in sorted({record["id"] for record in self.records} & plan.COMPARED):
            try:
                compared[identifier] = plan.compare(identifier, loaded.get((identifier, "baseline"), []),
                                                    loaded.get((identifier, "candidate"), []),
                                                    excluded.get(identifier, ()))
                if excluded.get(identifier) and compared[identifier].get("compared") is not False:
                    compared[identifier]["excluded_baseline"] = "indicative: baseline " + "; ".join(
                        excluded[identifier]) + " left out"
            except AuditError as error:
                compared[identifier] = {"compared": False, "reason": str(error)}
        return compared

    def index(self):
        entries, reasons = plan.recorded(self.records, self.options.candidate_only)
        compared = self.comparisons()
        document = {
            "commit": git("rev-parse", "HEAD"), "baseline_commit": self.options.baseline_commit,
            "steps": self.options.steps, "ids": self.options.ids, "candidate_only": self.options.candidate_only,
            "tags": {side: {"file": str(path), "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest(),
                            "images": self.tags[side]}
                     for side, path in (("candidate", self.options.candidate_tags),
                                        ("baseline", self.options.baseline_tags)) if path},
            "load_limit": plan.LOAD_LIMIT, "jobs": self.records,
            "baseline_harness": "the candidate's Rust harness files over the baseline's code, so those baseline "
                                "results read dirty_tree=true; they are compared, never recorded",
            "comparisons": compared, "recorded": entries, "not_recorded": reasons,
            "notes": dict(note.split("=", 1) for note in getattr(self.options, "note", None) or [])}
        path = self.keep.parent / ("gate9-" + self.stamp + ".json")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(document, indent=2) + "\n")
        regressions = ["{} {}".format(identifier, name) for identifier, figures in compared.items()
                       for name, figure in figures.items() if isinstance(figure, dict) and figure.get("regression")]
        return path, entries, reasons, regressions


def arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--candidate-tags", required=True, help="the candidate's tag file (TQA_*_IMAGE=sha256:...)")
    parser.add_argument("--baseline-tags", help="the previous release's tag file, for the compared measurements")
    parser.add_argument("--baseline-commit", help="the previous release's commit, for the compared measurements")
    parser.add_argument("--steps", help="comma-separated steps: a, b, c, d (KVM), e (24 h soak)")
    parser.add_argument("--ids", help="comma-separated IDs, each alone (PERF-010); with --steps, those steps' IDs too")
    parser.add_argument("--candidate-only", action="store_true",
                        help="compared IDs run once on the candidate, without the previous release: a reproduction, "
                             "never recorded as gate evidence")
    parser.add_argument("--dry-run", action="store_true", help="check and print each job; start nothing")
    parser.add_argument("--index-only", action="append", default=[], metavar="DIR[=ID,...]",
                        help="measure nothing: index the results kept in DIR (its jobs.jsonl, or rebuilt from its "
                             "files, whose load is then unknown); =ID,... takes only those IDs from DIR")
    parser.add_argument("--out", help="with --index-only: the directory the index is written to")
    parser.add_argument("--note", action="append", default=[], metavar="ID=TEXT",
                        help="a reviewer's note on an ID, written into the index as given")
    parser.add_argument("--kvm", action="store_true", help="allow step d (PERF-011 on microVMs)")
    parser.add_argument("--quiet-wait", type=int, default=3600, help="seconds a job waits for a quiet host")
    options = parser.parse_args(argv)
    if not options.steps and not options.ids and not options.index_only:
        parser.error("name --steps, --ids or both")
    return options


def split(text):
    return [item.strip() for item in (text or "").split(",") if item.strip()]


def main(argv=None):
    try:
        measure(arguments(sys.argv[1:] if argv is None else argv))
    except (AuditError, OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", None)
        raise SystemExit("Gate 9 run refused: {}{}".format(error, ": " + detail.strip() if detail else ""))


def rebuilt(directory, only=None):
    """The job records of a kept results directory: its jobs.jsonl, else rebuilt from its result files (named
    `<ID>-<round>-<side>-<file>`), each with its load unknown. A raw companion (no verdict) is checked against the
    SHA-256 its result names. `only` keeps those IDs."""
    directory = Path(directory).resolve()
    kept = directory / "jobs.jsonl"
    if kept.is_file():
        records = [json.loads(line) for line in kept.read_text().splitlines() if line.strip()]
        for record in records:
            for result in record["results"]:
                # Each result is kept beside jobs.jsonl, so a directory copied out of its tree still indexes.
                result["path"] = str(directory / Path(result["path"]).name)
        return [record for record in records if not only or record["id"] in only]
    jobs = {}
    for path in sorted(directory.glob("*.json")):
        data = json.loads(path.read_text())
        if not isinstance(data, dict) or "verdict" not in data:
            continue
        identifier, round_, side = re.match(r"^(PERF-\d{3}|SOAK)-(\d+)-(baseline|candidate)-", path.name).groups()
        if only and identifier not in only:
            continue
        raw = []
        for entry in plan.raw_files(data):
            target = path.parent / (path.name[:path.name.index(data["id"]) if data["id"] in path.name else 0]
                                    + entry["file"])
            sha256 = hashlib.sha256(target.read_bytes()).hexdigest() if target.is_file() else None
            raw.append({"path": str(target), "sha256": sha256, "matches": sha256 == entry["sha256"]})
        verdict = data.get("verdict", "fail")
        if any(not entry["matches"] for entry in raw):
            verdict = "fail"
        record = jobs.setdefault((identifier, round_, side), {
            "id": identifier, "side": side, "round": int(round_), "step": plan.step_of(identifier), "results": [],
            "exit": None, "disturbed": None, "max_load": None,
            "reason": "rebuilt from the kept results: the run's load record was lost"})
        record["results"].append({"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                  "id": data.get("id"), "verdict": verdict, "raw": raw})
    return sorted(jobs.values(), key=lambda record: (record["id"], record["round"]))


def index_only(options):
    """Indexes kept results without measuring: the comparisons and what may be recorded, from every source named."""
    runner = Runner(options, Path(tempfile.gettempdir()) / "tqa-gate9-index")
    if options.out:
        runner.keep = Path(options.out).resolve() / ("gate9-" + runner.stamp)
    for source in options.index_only:
        directory, _, ids = source.partition("=")
        runner.records.extend(rebuilt(directory, set(split(ids)) or None))
    path, entries, reasons, regressions = runner.index()
    print("Gate 9 index (no measuring) -> {}".format(path))
    for identifier, reason in sorted(reasons.items()):
        print("not recorded: {}: {}".format(identifier, reason))
    for regression in regressions:
        print("regression over 10 %: " + regression)
    for note in options.note:
        print("note: {}: {}".format(*note.split("=", 1)))
    if entries:
        print("python3 tools/gate-evidence.py 9 --run " + " ".join("--recorded " + entry for entry in entries))


def measure(options):
    if options.index_only:
        return index_only(options)
    jobs = plan.jobs(split(options.steps), kvm=options.kvm, ids=split(options.ids),
                     candidate_only=options.candidate_only)
    if options.dry_run:
        runner = Runner(options, Path(tempfile.gettempdir()) / "tqa-gate9-<run>")
        runner.check(jobs)
        print("Gate 9 dry run: {} jobs, nothing started".format(len(jobs)))
        for line in runner.describe(jobs):
            print("  " + line)
        return
    if any(job.step != "a" for job in jobs) and os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("steps b to e need Docker: set TQA_DOCKER_TESTS=1 and run with no other stack running")
    work = Path(tempfile.mkdtemp(prefix="tqa-gate9-"))
    runner = Runner(options, work)
    try:
        runner.check(jobs)
        runner.prepare(jobs)
        for job in jobs:
            runner.records.append(runner.run(job))
            runner.keep_record(runner.records[-1])
        path, entries, reasons, regressions = runner.index()
    finally:
        for tree in runner.trees.values():
            if tree.exists():
                git("worktree", "remove", "--force", str(tree))
        shutil.rmtree(work, ignore_errors=True)
    print("Gate 9 run -> {}".format(path))
    for identifier, reason in sorted(reasons.items()):
        print("not recorded: {}: {}".format(identifier, reason))
    for regression in regressions:
        print("regression over 10 %: " + regression)
    if entries:
        print("python3 tools/gate-evidence.py 9 --run " + " ".join("--recorded " + entry for entry in entries))
    if regressions or any(reason != plan.REPRODUCTION for reason in reasons.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()

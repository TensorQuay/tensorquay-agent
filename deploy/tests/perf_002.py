"""PERF-002 (TEST-PLAN §15) reproduced at any recorded rate and on either database, from the repository root:
    python3 deploy/tests/perf_002.py [--rate 1000]
        the database DATABASE_URL and TQA_DATABASE_URL name (the same one), such as the test cluster;
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_002.py --postgres disk [--rate 1000]
        a throwaway PostgreSQL with its defaults on a Docker volume (the image docker.yml pins), as an installation has
        it: every commit waits for the disk. It is removed afterwards, on a failure or a signal too.
Builds the release test binary, runs the one ignored test, and checks its result: the database's fsync,
synchronous_commit and full_page_writes are recorded (all on for `--postgres disk`), and the per-event lag file the
result names is beside it with the SHA-256 the result gives.
"""
import argparse
import hashlib
import json
import os
import re
import secrets
import subprocess
import sys
import time
from pathlib import Path

import compose
import gate9_plan as plan
from soak_audit import AuditError, require

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "tests" / "perf" / "results"
DURABILITY = ("fsync", "synchronous_commit", "full_page_writes")
# A fresh volume's first start (initdb, then a restart) on a loaded disk: container create alone has taken 44 s here.
READY_SECONDS = 180
RUN_SECONDS = 3600


def postgres_image(text):
    """The PostgreSQL image docker.yml pins, so the disk run measures the database an installation runs."""
    match = re.search(r"^\s+image:\s*(postgres:\S+@sha256:[0-9a-f]{64})\s*$", text, flags=re.MULTILINE)
    require(match is not None, "deploy/compose/docker.yml pins no postgres image by digest")
    return match[1]


def check_result(path, disk):
    """The result's database durability and its lag file; returns the result. A disk run needs every setting on."""
    result = json.loads(path.read_text())
    database = result.get("environment", {}).get("database", {})
    missing = [name for name in DURABILITY if name not in database]
    require(not missing, "{} does not record the database's {}".format(path.name, ", ".join(missing)))
    if disk:
        off = [name for name in DURABILITY if database[name] != "on"]
        require(not off, "the disk database ran with {} off".format(", ".join(off)))
    named = plan.raw_files(result)
    require(any(entry.get("name") == "lag" for entry in named), "{} names no lag file".format(path.name))
    for entry in named:
        raw = path.parent / entry["file"]
        require(raw.is_file(), "{} is missing beside its result".format(entry["file"]))
        require(hashlib.sha256(raw.read_bytes()).hexdigest() == entry["sha256"],
                "{} changed since its result named it".format(entry["file"]))
    return result


def snapshot():
    return {str(path.relative_to(RESULTS)) for path in RESULTS.rglob("*.json")} if RESULTS.is_dir() else set()


class DiskPostgres:
    """A PostgreSQL container with its defaults on its own volume, on a loopback port; removed on every exit, a failed
    start included."""
    def __init__(self, env):
        self.name = "tqa-perf-002-{}".format(os.getpid())
        password = secrets.token_urlsafe(24)
        # `-e POSTGRES_PASSWORD` takes the value from this environment, so it never appears on a command line;
        # TQA_POSTGRES_PASSWORD holds the same value so diagnostics redact it.
        self.env = dict(env, TQA_POSTGRES_PASSWORD=password, POSTGRES_PASSWORD=password)

    def __enter__(self):
        try:
            return self.start()
        except BaseException:
            self.__exit__()
            raise

    def start(self):
        image = postgres_image((ROOT / "deploy" / "compose" / "docker.yml").read_text())
        compose.command(["docker", "run", "-d", "--name", self.name, "--label", "org.tensorquay.perf=PERF-002",
                         "-p", "127.0.0.1::5432", "--mount", "type=volume,source={},target=/var/lib/postgresql"
                         .format(self.name), "--shm-size", "256m", "-e", "POSTGRES_USER=tqa", "-e",
                         "POSTGRES_DB=gateway", "-e", "POSTGRES_PASSWORD", image, "-c", "max_connections=300"],
                        self.env, timeout=300)
        started = time.monotonic()
        while subprocess.run(["docker", "exec", self.name, "pg_isready", "-U", "tqa", "-d", "gateway"],
                             env=self.env, capture_output=True).returncode:
            state = subprocess.run(["docker", "inspect", "-f", "{{.State.Status}}", self.name], env=self.env,
                                   capture_output=True, text=True).stdout.strip()
            if state in ("exited", "dead"):
                logs = subprocess.run(["docker", "logs", "--tail", "15", self.name], env=self.env, capture_output=True,
                                      text=True)
                require(False, "PostgreSQL exited before it was ready:\n" + compose.redact(logs.stdout + logs.stderr,
                                                                                         self.env).strip())
            require(time.monotonic() - started < READY_SECONDS,
                    "PostgreSQL was not ready within {} s".format(READY_SECONDS))
            time.sleep(1)
        print("PostgreSQL ready in {:.0f} s".format(time.monotonic() - started), flush=True)
        port = compose.command(["docker", "port", self.name, "5432/tcp"], self.env, capture=True).stdout
        port = port.split()[0].rsplit(":", 1)[1]
        return "postgres://tqa:{}@127.0.0.1:{}/gateway".format(self.env["TQA_POSTGRES_PASSWORD"], port)

    def __exit__(self, *_exception):
        for args in (["docker", "rm", "-f", "--volumes", self.name], ["docker", "volume", "rm", "-f", self.name]):
            subprocess.run(args, env=self.env, capture_output=True, timeout=120)
        print("removed PostgreSQL container and volume {}".format(self.name), flush=True)


def measure(options, env):
    """Builds, runs the test once and checks the result it wrote; returns (result path, result)."""
    env = dict(env, SQLX_OFFLINE="true", TQA_PERF_002_RATE=str(options.rate))
    subprocess.run(plan.rust_command("PERF-002", build=True), cwd=ROOT, env=env, check=True)
    before = snapshot()
    exit_code = subprocess.run(plan.rust_command("PERF-002", build=False), cwd=ROOT, env=env,
                               timeout=RUN_SECONDS).returncode
    written = [name for name in plan.new_files(before, snapshot()) if Path(name).name.startswith("PERF-002-")]
    require(len(written) == 1, "the run (exit {}) wrote {} PERF-002 results, not one".format(exit_code,
                                                                                            len(written)))
    path = RESULTS / written[0]
    return path, check_result(path, options.postgres == "disk"), exit_code


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rate", type=int, default=10_000, help="events a second, whole thousands (default 10000)")
    parser.add_argument("--postgres", choices=("given", "disk"), default="given")
    options = parser.parse_args(argv)
    compose.teardown_on_signals()
    env = os.environ.copy()
    try:
        if options.postgres == "disk":
            require(env.get("TQA_DOCKER_TESTS") == "1", "--postgres disk starts a container: set TQA_DOCKER_TESTS=1")
            with DiskPostgres(env) as url:
                path, result, exit_code = measure(options, dict(env, DATABASE_URL=url, TQA_DATABASE_URL=url))
        else:
            require(env.get("DATABASE_URL") and env.get("DATABASE_URL") == env.get("TQA_DATABASE_URL"),
                    "set DATABASE_URL and TQA_DATABASE_URL to the same database, or pass --postgres disk")
            path, result, exit_code = measure(options, env)
    except (AuditError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit("PERF-002 run refused: {}".format(error))
    database = result["environment"]["database"]
    lag = result["result"]["lag_ms"]
    print("PERF-002 {}/s, {} PostgreSQL {} ({}): lag p50 {:.2f} ms, p99 {:.2f} ms, {} -> {}".format(
        options.rate, options.postgres, database.get("server_version"), ", ".join(
            "{}={}".format(name, database[name]) for name in DURABILITY), lag["p50"], lag["p99"],
        result["verdict"], path.relative_to(ROOT)), flush=True)
    if exit_code or result["verdict"] != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    sys.exit(main())

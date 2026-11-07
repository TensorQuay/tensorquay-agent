"""The 500-start reproduction of c14b-rc's VM host crash (exit 139 about 0.2 s into a start, before the runtime
registered, 1 start in 31; `docs/evals/gate-8.md`, finding 1). Two modes, each on a disposable microVM stack
(`stack.run`), each counting every start's end instead of stopping at the first failure:

    TQA_DOCKER_TESTS=1 python3 deploy/firecracker/start_repro.py product --starts 500 --out <dir>
    TQA_DOCKER_TESTS=1 python3 deploy/firecracker/start_repro.py bare --starts 500 --out <dir>

`product`: each start is a new task's first upload (as PERF-011's upload sample), which starts a VM; then the task is
deleted and its VM host is waited out. A failed start is recorded and the loop goes on; it stops early only after
`MAX_CONSECUTIVE_FAILURES` failures in a row, so a broken stack isn't hammered. The engine's own record of every VM
host's end (`die` with its exit code) and every VM host's log come from the stack's recorder; the gateway's WARN lines
say what it saw (SBX-F-038 names a crash's step or Firecracker's code).

`bare`: one task's VM host is cloned from `docker inspect`: the same image, user, capabilities, devices, limits and
network, with copies of its `/vm` files (made by a helper container, so owners and modes stay), and none of the
`tqa.*` labels, so the gateway never adopts or removes a clone. Each clone runs for `BARE_WINDOW_SECONDS`, then is
removed; one that ended by itself is a start's end to classify. This separates the launcher, `ip`, `nft`, `setpriv`
and Firecracker from the gateway's load. The copies stay in the stack's private directory and go with it.

The outputs, in `--out`: `summary.json` (each start, each VM host's end and the verdict), the recorder's VM host logs
and engine events, and the gateway's WARN lines; all redacted. No value of a token or key is written.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stack as vm_stack  # noqa: E402
import compose  # noqa: E402  (deploy/tests, on the path stack.py set)

STARTS = 500
MAX_CONSECUTIVE_FAILURES = 10
START_WITHIN = 60
BARE_WINDOW_SECONDS = 3.0
# A bare signal death (128 + SIGSEGV) and Firecracker's own fault exits (FcExitCode, v1.17: 148 seccomp, 149 SIGBUS,
# 150 SIGSEGV ... 157): the crash being reproduced. 137 and 143 are the kill and the stop a delete sends.
CRASH_CODES = {139} | set(range(148, 158))
STOP_CODES = {137, 143}
# Labels a clone keeps none of: with them, the gateway would treat it as one of its own VM hosts.
OWN_LABEL_PREFIX = "tqa."
REPLY = {"events": [{"type": "text", "text": "ready"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}


def classify(code):
    """What a VM host's exit code means for the reproduction."""
    if code is None:
        return "alive"
    if code in CRASH_CODES:
        return "crash"
    if code in STOP_CODES:
        return "stopped"
    return "clean" if code == 0 else "other"


def vm_host_ends(events_text):
    """Every VM host's end in the engine's events (`docker events --format '{{json .}}'`): a `die` of a container that
    carries a conversation label, so the backend's short-lived helpers are left out."""
    ends = []
    for line in events_text.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        actor = event.get("Actor") or {}
        attributes = actor.get("Attributes") or {}
        if event.get("Type") != "container" or event.get("Action") != "die" or "tqa.conversation" not in attributes:
            continue
        code = int(attributes["exitCode"]) if str(attributes.get("exitCode", "")).lstrip("-").isdigit() else None
        ends.append({"container": actor.get("ID", "")[:12], "conversation": attributes["tqa.conversation"],
                     "exit_code": code, "class": classify(code), "time_nano": event.get("timeNano")})
    return ends


def summarize(mode, starts, ends, target):
    """The run's counts and verdict. `starts` are the loop's own records; `ends` the VM hosts' ends."""
    by_class, by_code = {}, {}
    for end in ends:
        by_class[end["class"]] = by_class.get(end["class"], 0) + 1
        key = "alive" if end["exit_code"] is None else str(end["exit_code"])
        by_code[key] = by_code.get(key, 0) + 1
    crashes = [end for end in ends if end["class"] == "crash"]
    failed = [start for start in starts if not start["ok"]]
    return {
        "mode": mode, "target_starts": target, "starts": len(starts), "succeeded": len(starts) - len(failed),
        "failed": len(failed), "failures": failed, "vm_host_ends": len(ends), "ends_by_class": by_class,
        "ends_by_exit_code": by_code, "crashes": crashes,
        "stopped_early": len(starts) < target,
        "verdict": ("reproduced: {} crash(es) in {} starts".format(len(crashes), len(starts)) if crashes
                    else "not reproduced in {} starts".format(len(starts))),
    }


def clone_command(record, files, name):
    """`docker run -d` arguments for a clone of an inspected VM host: its request with `/vm`'s writable files and
    sandbox files taken from `files` (a directory of copies), and the read-only artefacts (kernel, rootfs) shared."""
    config, host = record["Config"], record["HostConfig"]
    args = ["docker", "run", "-d", "--name", name, "--user", config.get("User") or "0:0",
            "--label", "tqa-repro.bare=1", "--restart", "no"]
    if host.get("ReadonlyRootfs"):
        args.append("--read-only")
    for capability in host.get("CapDrop") or []:
        args += ["--cap-drop", capability]
    for capability in host.get("CapAdd") or []:
        args += ["--cap-add", capability]
    for option in host.get("SecurityOpt") or []:
        args += ["--security-opt", option]
    for device in host.get("Devices") or []:
        args += ["--device", "{}:{}:{}".format(device["PathOnHost"], device["PathInContainer"],
                                               device.get("CgroupPermissions") or "rwm")]
    for key, value in sorted((host.get("Sysctls") or {}).items()):
        args += ["--sysctl", "{}={}".format(key, value)]
    for target, options in sorted((host.get("Tmpfs") or {}).items()):
        args += ["--tmpfs", "{}:{}".format(target, options) if options else target]
    if host.get("Memory"):
        args += ["--memory", str(host["Memory"]), "--memory-swap", str(host.get("MemorySwap") or host["Memory"])]
    if host.get("NanoCpus"):
        args += ["--cpus", "{:.3f}".format(host["NanoCpus"] / 1e9)]
    if host.get("PidsLimit"):
        args += ["--pids-limit", str(host["PidsLimit"])]
    args += ["--network", host.get("NetworkMode") or "none"]
    for mount in record.get("Mounts") or []:
        target = mount["Destination"]
        copied = Path(files) / Path(target).name
        source = copied if copied.exists() else Path(mount["Source"])
        args += ["--mount", "type=bind,source={},target={}{}".format(source, target,
                                                                    "" if mount.get("RW") else ",readonly")]
    for key in sorted(config.get("Labels") or {}):
        if not key.startswith(OWN_LABEL_PREFIX):
            args += ["--label", "{}={}".format(key, config["Labels"][key])]
    return args + [config["Image"]] + list(config.get("Cmd") or [])


# What `bare` copies: everything under /vm but the shared, read-only kernel and rootfs.
COPIED = ("workspace.img", "state.img", "config.img", "exit.img", "vm.json", "rules.nft")


def copy_files(record, directory, env):
    """Copies the VM host's own `/vm` files into `directory` with a helper container (the VM host image, as root of
    the engine's namespace), so their owners and modes are the VM host's."""
    directory.mkdir(mode=0o700)
    mounts = {Path(mount["Destination"]).name: mount["Source"] for mount in record.get("Mounts") or []}
    args = ["docker", "run", "--rm", "--network", "none", "--user", "0:0", "--entrypoint", "cp",
            "--mount", "type=bind,source={},target=/out".format(directory)]
    for name in COPIED:
        args += ["--mount", "type=bind,source={},target=/in/{},readonly".format(mounts[name], name)]
    compose.command(args + [record["Config"]["Image"], "-a"] + ["/in/" + name for name in COPIED] + ["/out/"],
                    env, timeout=300)


def upload_start(stack):
    """One start: a new task's first upload, which starts its VM. Returns the start's record; never raises for a
    refused or timed-out start."""
    began = time.monotonic()
    record = {"at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="milliseconds"), "ok": False}
    try:
        conversation = stack.request("/api/v1/conversations", {"title": "start-repro", "model_route": "local"})["id"]
        record["conversation"] = conversation
        request = urllib.request.Request(
            stack.origin + "/api/v1/conversations/{}/uploads?name=data.csv".format(conversation), data=b"a,b\n1,2\n",
            method="POST", headers={"Origin": stack.origin, "Content-Type": "application/octet-stream"})
        with stack.client.open(request, timeout=START_WITHIN) as response:
            response.read(65536)
            record["ok"] = response.status == 201
            record["status"] = response.status
    except urllib.error.HTTPError as error:
        record["status"], record["body"] = error.code, error.read(2048).decode("utf-8", "replace")
    except (OSError, ValueError, KeyError) as error:
        record["error"] = type(error).__name__ + ": " + str(error)[:300]
    record["ms"] = round((time.monotonic() - began) * 1000, 1)
    if "conversation" in record:
        try:
            delete_task(stack, record["conversation"])
        except (OSError, AssertionError, urllib.error.URLError) as error:
            record["delete_error"] = str(error)[:300]
    return record


def delete_task(stack, conversation, seconds=120):
    """Deletes the task and waits (bounded) until its VM host is gone, so images and hosts don't pile up."""
    request = urllib.request.Request(stack.origin + "/api/v1/conversations/" + conversation, method="DELETE",
                                     headers={"Origin": stack.origin, "Idempotency-Key": os.urandom(16).hex()})
    with stack.client.open(request, timeout=30) as response:
        if response.status not in (200, 202, 204):
            raise AssertionError("delete answered {}".format(response.status))
    deadline = time.monotonic() + seconds
    while compose.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation], stack.env,
                          capture=True).stdout.split():
        if time.monotonic() > deadline:
            raise AssertionError("the VM host outlived its task by {} s".format(seconds))
        time.sleep(0.5)


def loop(starts, start, out, stop_after=MAX_CONSECUTIVE_FAILURES):
    """Runs `start(index)` up to `starts` times, appending each record to `out/starts.jsonl` as it comes; stops after
    `stop_after` failures in a row. Returns the records."""
    records, consecutive = [], 0
    with (out / "starts.jsonl").open("a") as journal:
        for index in range(starts):
            record = dict(start(index), index=index)
            records.append(record)
            journal.write(json.dumps(record) + "\n")
            journal.flush()
            consecutive = 0 if record["ok"] else consecutive + 1
            if consecutive >= stop_after:
                print("start-repro: {} failed starts in a row; stopping".format(consecutive), flush=True)
                break
            if (index + 1) % 50 == 0:
                print("start-repro: {} starts, {} failed".format(index + 1, sum(not r["ok"] for r in records)),
                      flush=True)
    return records


def bare_start(record, files, env, index):
    """One clone's run: started, given `BARE_WINDOW_SECONDS`, inspected, then removed. Its end, and its log's last
    lines if it ended by itself."""
    name = "tqa-repro-bare-{}-{}".format(os.getpid(), index)
    result = {"at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="milliseconds"), "name": name}
    try:
        compose.command(clone_command(record, files, name), env, capture=True, timeout=60)
        time.sleep(BARE_WINDOW_SECONDS)
        state = json.loads(compose.command(["docker", "inspect", "--format", "{{json .State}}", name], env,
                                           capture=True, timeout=30).stdout)
        code = None if state.get("Running") else state.get("ExitCode")
        result.update(exit_code=code, **{"class": classify(code)})
        result["ok"] = result["class"] in ("alive", "clean")
        if code is not None:
            logs = compose.command(["docker", "logs", "--tail", "40", name], env, capture=True, timeout=30)
            result["log_tail"] = compose.redact(logs.stdout + logs.stderr, env)[-4000:]
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        result.update(ok=False, error=compose.redact(str(error), env)[:300])
    finally:
        subprocess.run(["docker", "rm", "-f", name], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=60, check=False)
    return result


def keep_evidence(stack, out):
    """Copies the recorder's VM host logs and engine events, and the gateway's WARN lines, into `out`, redacted."""
    recording = Path(stack.directory).parent / "vm-hosts"
    target = out / "vm-hosts"
    target.mkdir(exist_ok=True)
    for path in sorted(recording.glob("*")) if recording.is_dir() else []:
        (target / path.name).write_text(compose.redact(path.read_text(errors="replace"), stack.env))
    with (out / "gateway-warnings.txt").open("w") as output:
        vm_stack.dump_gateway_warnings(stack.configuration, stack.env, out=lambda text: output.write(text + "\n"))
    events = target / "events"
    return events.read_text(errors="replace") if events.exists() else ""


def product(stack, starts, out):
    stack.sign_in()
    records = loop(starts, lambda _index: upload_start(stack), out)
    ends = vm_host_ends(keep_evidence(stack, out))
    return summarize("product", records, ends, starts)


def bare(stack, starts, out):
    stack.sign_in()
    conversation = stack.request("/api/v1/conversations", {"title": "start-repro-template", "model_route": "local"})["id"]
    request = urllib.request.Request(
        stack.origin + "/api/v1/conversations/{}/uploads?name=data.csv".format(conversation), data=b"a,b\n1,2\n",
        method="POST", headers={"Origin": stack.origin, "Content-Type": "application/octet-stream"})
    with stack.client.open(request, timeout=START_WITHIN) as response:
        response.read(65536)
    [host] = compose.command(["docker", "ps", "-q", "--filter", "label=tqa.conversation=" + conversation], stack.env,
                             capture=True).stdout.split()
    record = json.loads(compose.command(["docker", "inspect", host], stack.env, capture=True).stdout)[0]
    files = Path(stack.directory).parent / "repro-files"
    copy_files(record, files, stack.env)
    delete_task(stack, conversation)
    records = loop(starts, lambda index: bare_start(record, files, stack.env, index), out)
    keep_evidence(stack, out)
    return summarize("bare", records, bare_ends(records), starts)


def bare_ends(records):
    """The clones' ends, in `vm_host_ends`' shape: a clone still running at the window's end is `alive`."""
    return [{"container": record["name"], "conversation": None, "exit_code": record.get("exit_code"),
             "class": record.get("class", "unknown"), "time_nano": None}
            for record in records if "class" in record]


def arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("mode", choices=("product", "bare"))
    parser.add_argument("--starts", type=int, default=STARTS)
    parser.add_argument("--out", type=Path, required=True)
    options = parser.parse_args(argv)
    if options.starts < 1:
        parser.error("--starts must be at least 1")
    return options


def main(argv=None):
    options = arguments(argv)
    options.out.mkdir(parents=True, exist_ok=False)
    result = {}

    def check(stack):
        result.update((product if options.mode == "product" else bare)(stack, options.starts, options.out))

    vm_stack.run("START-REPRO-" + options.mode, {"model": "fixture", "responses": [REPLY]}, check)
    (options.out / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print("start-repro {}: {}".format(options.mode, result.get("verdict", "no result (the stack failed)")), flush=True)
    return 0 if result else 1


if __name__ == "__main__":
    sys.exit(main())

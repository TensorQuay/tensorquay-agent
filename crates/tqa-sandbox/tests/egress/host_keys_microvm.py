"""SEC-047 and SEC-049's microVM half (TEST-PLAN §19a, "SEC-019 has no microVM half for host keys"): the host-keys
fixture of `host_keys.py` with the sandbox a microVM, so a keyed request leaves the guest through its tap, not the
Docker sandbox network.

One task, two runs, one guest:
1. a bearer key for the keyed host's exact address: a call with the run's placeholder, then the run's variable; then a
   search of the guest (below);
2. that key replaced by a `basic` key for the host's `/team` prefix: plain git clones, commits and pushes inside it with
   only the settings the run is given, and a push to `/other` with the placeholder as the URL's password is refused;
   then a second search of the guest. (If the VM was retired at idle between the runs, the second search is of the new
   guest; the result says which.)

What passes:
- the host received the value for the bearer call, once, and for Git only inside `/team`; the task saw only the
  placeholder, the host's echo included (the proxy scrubs answers); the outside push is refused `wrong_host`;
- **the guest:** a guest has no `docker exec`, so a scripted shell turn searches it, given only hashes of the value's
  forms (`deploy/firecracker/guest_search.py`): every process's environment, command line and memory-map listing, and
  the writable trees. Its positive controls (the runtime token, the runtime's environment, a value planted in two
  halves) must be found;
- **outside the guest:** the value is in no event, no log (gateway, model, database, the VM host's, whose console is the
  guest's), no plaintext table and no file of the installation's storage (the task's workspace and state images among
  them), searched by a helper container that is given the value on its standard input; that search must find a
  control the run wrote into the workspace image (the clone's origin, in `.git/config`).

Run from the repository root, on a prepared KVM host with the Firecracker stack's variables (as `check.sh --kvm`) and
images built from the commit under test:
    PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE=sha256:<gateway image id> \\
    TQA_RUNTIME_TEST_IMAGE=<workstation runtime image> python3 -m egress.host_keys_microvm
"""
import json
import os
import signal
import subprocess
import sys
import tempfile

from .host_keys import GIT_NAME, GIT_SCRIPT, NAME, HostKeyStack, finished, results
from .stack import ROOT, compose

sys.path.insert(0, str(ROOT / "deploy/firecracker"))
import guest_search  # noqa: E402

# A microVM's first run may build the runtime's root filesystem before the stack starts.
DEADLINE_SECONDS = 1800
PHRASE = "Use the keys inside a microVM"
# The storage search's control: the clone's origin path, which git writes into the workspace image's .git/config.
CONTROL = "/team/repo.git"


def scripted(when, events):
    return {"when": when, "events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


def shell(command):
    return [{"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 1}]


def responses(address, authority, value):
    """The task's two runs, in order, each ending with its own search of the guest for the value's forms (hashes
    only, a fresh planted control each)."""
    labelled = [("form:{}".format(index), form) for index, form in enumerate(guest_search.forms(value))]
    call = 'curl -sS -w "\\nstatus %{{http_code}}\\n" -H "Authorization: Bearer {}" ' + address + "/echo"
    return [
        scripted(PHRASE, shell(call.format("${" + NAME + "}"))),
        scripted(PHRASE, shell("env | grep -E '^" + NAME + "=' || true")),
        scripted(PHRASE, shell(guest_search.shell(labelled))),
        scripted(PHRASE, [{"type": "text", "text": "The keyed call is done."}]),
        scripted(PHRASE, shell(GIT_SCRIPT.format(origin=address, authority=authority))),
        scripted(PHRASE, shell(guest_search.shell(labelled))),
        scripted(PHRASE, [{"type": "text", "text": "The Git work is done."}]),
    ]


class MicrovmHostKeyStack(HostKeyStack):
    """The host-keys fixture with the sandbox a microVM and only this case's script."""
    backend = "firecracker"

    def scenario(self):
        super().scenario()  # the fixture's address, its authority file and `ca_file`
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json", json.dumps({
            "model": "fixture", "select": "prompt",
            "responses": responses(self.address, self.address[len("https://"):], self.value)}))
        compose.ownership(self.directory, self.env, "1000:1000")

    def vm_hosts(self, conversation):
        return self.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation]).split()

    def storage_hits(self, needles):
        """Which files of the installation's storage hold each needle: a read-only helper container in the runtime
        image greps the tree, the needle on its standard input (never in an argument)."""
        storage = self.env["TQA_STORAGE_ROOT"]
        hits = {}
        for label, needle in needles:
            done = subprocess.run(
                ["docker", "run", "--rm", "-i", "--network", "none", "--user", "0:0", "--read-only",
                 "--entrypoint", "grep", "--mount", "type=bind,source={},target=/s,readonly".format(storage),
                 self.runtime, "-rlaF", "-f", "-", "/s"],
                input=needle + "\n", env=self.env, capture_output=True, text=True, timeout=900, check=False)
            assert done.returncode in (0, 1), "the storage search failed: " + self.redact(done.stderr[-500:])
            hits[label] = done.stdout.split()
        return hits


def leaks(stack, stored, conversation):
    """Every place outside the guest that holds the value; asserts the storage search's control."""
    found = []
    if stack.value in json.dumps(stored):
        found.append("the task's events")
    for service in ("gateway", "fake-model", "postgres"):
        if stack.value in stack.command(stack.configuration + ["logs", "--no-color", service]):
            found.append(service + " log")
    for host in stack.vm_hosts(conversation):
        if stack.value in stack.command(["docker", "logs", host]):
            found.append("the VM host's log ({})".format(host[:12]))
    dump = stack.command(stack.configuration + ["exec", "-T", "postgres", "pg_dump", "-U", "tqa", "-d", "gateway",
                                                "--data-only"])
    if stack.value in dump:
        found.append("a plaintext table")
    hits = stack.storage_hits([("value", stack.value), ("control", CONTROL)])
    # The task's workspace image (`workspaces/<task>.ext4`): found there, the search reads inside the images.
    assert any(path.startswith("/s/workspaces/") for path in hits["control"]), \
        "the storage search did not find the clone's origin in the workspace image: {}".format(hits["control"])
    found += ["storage: " + path for path in hits["value"]]
    return found


def check(stack):
    conversation, path, stream = stack.task("Host keys in a microVM", PHRASE)
    first = results(finished(stack, path, stream))
    # Listed once the first run has finished: right after the task is made, its VM host may not exist yet.
    hosts = stack.vm_hosts(conversation)
    assert len(hosts) == 1, "one VM host for the task: {}".format(hosts)
    assert len(first) == 3 and all(result["status"] == "ok" for result in first), first
    echo, environment = first[0]["content"], first[1]["content"]
    placeholder = environment.strip().split("=", 1)[1] if "=" in environment else ""
    assert placeholder.startswith("tqa_ph_"), "the run holds the placeholder: " + environment
    assert "Bearer " + placeholder in echo, "the host's echo is scrubbed back to the placeholder: " + echo
    assert stack.received() == ["Bearer " + stack.value], "the host received the value, once (the proxy swapped it)"
    problems = guest_search.problems(guest_search.report(first[2]["content"]))
    assert not problems, "after the bearer call: {}".format(problems)

    stack.request("/api/v1/host-keys/{}?version={}".format(stack.key["id"], stack.key["version"]), method="DELETE")
    key = stack.request("/api/v1/host-keys", {"name": GIT_NAME, "address": stack.address + "/team",
                                              "scheme": "basic", "user_name": "git", "value": stack.value})
    before = len(stack.git_requests())
    stack.request(path + "/messages", {"text": "Now clone and push.", "attachments": []})
    stored = finished(stack, path, stream)
    second = results(stored)[len(first):]
    assert len(second) == 2, second
    output = second[0]["content"]
    assert "pushed 1" in output, "{}\nthe host's Git requests: {}".format(output, stack.git_requests()[before:])
    assert "outside: error: 403" in output, output
    requests = stack.git_requests()[before:]
    authorised = [request for request in requests if request["authorised"]]
    assert authorised and all(request["path"].startswith("/team/") for request in authorised), requests
    assert any(request["path"].endswith("/git-receive-pack") and request["method"] == "POST"
               for request in authorised), requests
    assert all(not request["authorised"] for request in requests if request["path"].startswith("/other/")), requests
    uses = {}
    for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(key["id"]))["items"]:
        uses[row["outcome"]] = uses.get(row["outcome"], 0) + row["count"]
    assert uses.get("wrong_host") == 1 and uses.get("ok", 0) >= 3 and set(uses) == {"ok", "wrong_host"}, uses
    report = guest_search.report(second[1]["content"])
    problems = guest_search.problems(report)
    assert not problems, "after the push: {}".format(problems)
    # Each run's guest was searched at its end; whether the second run kept the first one's VM is said, not required
    # (an idle retire between the runs starts a new one).
    same = stack.vm_hosts(conversation) == hosts

    found = leaks(stack, stored, conversation)
    assert not found, "the value leaked into: " + ", ".join(found)
    print("SEC-047 and SEC-049 on the microVM passed ({}): a bearer call and a plain-git clone "
          "and push through the proxy with the person's keys; the host got the value only inside the key's address "
          "(Git: {} authorised requests, all under /team; /other refused wrong_host); the task saw only placeholders; "
          "the value is absent from the guest's processes and {} files (the search found its three controls), the "
          "events, every log, the plaintext tables and the installation's storage (whose search found the clone's "
          "origin in the workspace image)".format(
              "both runs in VM host " + hosts[0][:12] if same else "the second run in a new guest",
              len(authorised), report["files"]), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("The microVM host-keys fixture was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-host-keys-microvm-")
    directory = compose.installation(temporary)
    stack, failure = MicrovmHostKeyStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("host-keys")
        stack.start()
        check(stack)
    except BaseException as error:
        failure = error
        print("Host-keys microVM contract failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()

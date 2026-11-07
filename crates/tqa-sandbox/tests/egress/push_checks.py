"""The push fixture's origin witness and the credential searches (SEC-019), for the suites that push with plain git
and a host key (tests/e2e/gate4.py, web_key_use.py)."""
import json
import subprocess

from .web_connector import forms


# Counts files under one root that hold any form of the credential (a JSON list on stdin, as git_push.SEARCH takes).
SEARCH_ROOT = (
    "import json, os, sys\n"
    "forms = [form.encode() for form in json.load(sys.stdin)]\n"
    "count = hits = 0\n"
    "for directory, _names, files in os.walk('{root}', followlinks=False):\n"
    "    for name in files:\n"
    "        count += 1\n"
    "        try:\n"
    "            with open(os.path.join(directory, name), 'rb') as source:\n"
    "                data = source.read()\n"
    "                hits += any(form in data for form in forms)\n"
    "        except OSError:\n"
    "            pass\n"
    "print('searched', count, 'files; hits', hits)\n")


def witness(stack):
    """Origin requests made after the fixture's own checkout."""
    return stack.upstream_get("/witness")[stack.seeded:]


def receive_packs(stack):
    return [item for item in witness(stack)
            if item["method"] == "POST" and item["path"].endswith("/git-receive-pack")]


def sandbox_run(stack, sandbox, script, stdin=None):
    return subprocess.run(["docker", "exec", "-i", "-w", "/workspace", sandbox, "sh", "-c", script], input=stdin,
                          env=stack.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=60)


def storage_hits(stack):
    """Searches host storage through a read-only mount, so the search changes no ownership."""
    found = subprocess.run(["docker", "run", "--rm", "-i", "--network", "none", "--read-only", "--mount",
                            "type=bind,source={},target=/storage,readonly".format(stack.env["TQA_STORAGE_ROOT"]),
                            "--entrypoint", "python3", stack.runtime, "-c", SEARCH_ROOT.format(root="/storage")],
                           input=json.dumps(forms(stack.secret)), env=stack.env, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE,
                           text=True, timeout=120)
    words = found.stdout.split()
    assert found.returncode == 0 and len(words) == 5 and words[0] == "searched", "The storage search failed"
    return int(words[4])

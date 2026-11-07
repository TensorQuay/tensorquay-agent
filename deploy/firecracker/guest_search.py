"""SEC-013 and SEC-019 on the microVM: a search inside the guest for a credential it is never given.

A guest has no `docker exec`, so the search runs through the agent's own shell tool, and whatever that tool is given
lands in the transcript, one of the places the credential must be absent from. So the guest gets, for each form of the
value, only its length, a keyed rolling hash (Rabin-Karp modulo 2^61 - 1 with a per-run random base) and a salted
SHA-256. It slides over what it reads, checks each window's rolling hash, and confirms a match with the digest. It
prints only the places and labels of the hits.

What it looks for (forms()): the value raw and URL-encoded, base64 of the whole value, and base64 of anything holding it
(an HTTP Basic header's `user:token`, whatever the user name, among them): for each of the three byte offsets at which
the value can start, the run of base64 characters that depends on the value's bytes alone, in both alphabets.

Where it looks: every process's environment, command line and memory-map listing, and the files of the guest's
writable trees. The root filesystem is read-only in the guest and was built before any credential existed. Process
memory itself is not read: that needs ptrace across users; nor more than 16 MiB of one file. Three positive controls
show that the search saw something: the runtime token, which it reads from its file itself and which must be found
there; `TQA_GATEWAY_URL=` in some process's environment; and a value the scripted turn plants in two halves, so it is
never whole in the command or the transcript, and which the search is given as hashes only, so only the hash matcher can
find it: a broken matcher fails the run.
"""
import base64
import hashlib
import json
import secrets
import urllib.parse

MARK = "guest-search: "
MODULUS = (1 << 61) - 1
ROOTS = ["/workspace", "/tmp", "/run", "/home", "/var/tmp"]
CONTROL_PATH = "/tmp/hashed-control"
# Below this the shortest aligned base64 run falls under 32 characters, short enough to match by chance.
MIN_LENGTH = 26


def forms(value):
    """Every form of `value` a search looks for (module docstring), each at least 32 characters long."""
    if len(value) < MIN_LENGTH:
        raise ValueError("a credential to search for must be at least {} characters".format(MIN_LENGTH))
    raw = value.encode()
    whole = [base64.b64encode(raw).decode(), base64.urlsafe_b64encode(raw).decode()]
    found = {value, urllib.parse.quote(value, safe=""), *whole, *(item.rstrip("=") for item in whole)}
    for offset in range(3):
        first, end = -(-8 * offset // 6), 8 * (offset + len(raw)) // 6
        for encode in (base64.b64encode, base64.urlsafe_b64encode):
            found.add(encode(bytes(offset) + raw).decode()[first:end])
    return sorted(found)


def control_value():
    return "hashed-control-" + secrets.token_hex(16)


def plant(control, path=CONTROL_PATH):
    """Shell that writes the control in two halves, so the command never holds it whole."""
    half = len(control) // 2
    return "printf '%s%s' '{}' '{}' > {}\n".format(control[:half], control[half:], path)


def rolling(data, base):
    value = 0
    for byte in data:
        value = (value * base + byte) % MODULUS
    return value


def spec(labelled, *, control=None, roots=None, proc="/proc", token_file="/run/tqa/token"):
    """What the guest is given for the (label, value) pairs and the planted control: hashes only, with this run's own
    salt and base."""
    salt, base = secrets.token_bytes(16), secrets.randbelow(MODULUS - 257) + 257
    labelled = list(labelled) + ([("control:hashed", control)] if control else [])
    needles = [{"label": label, "length": len(value.encode()), "rolling": rolling(value.encode(), base),
                "digest": hashlib.sha256(salt + value.encode()).hexdigest()} for label, value in labelled]
    return {"salt": salt.hex(), "base": base, "needles": needles, "roots": list(roots or ROOTS), "proc": proc,
            "token_file": token_file}


PROGRAM = r'''
import hashlib, json, os
spec = json.loads(SPEC)
M = (1 << 61) - 1
base, salt = spec["base"], bytes.fromhex(spec["salt"])
LIMIT = 16 * 1024 * 1024
literal = [("control:gateway", b"TQA_GATEWAY_URL=")]
try:
    with open(spec["token_file"], "rb") as source:
        literal.append(("control:token", source.read().strip()))
except OSError:
    pass
groups = {}
for needle in spec["needles"]:
    groups.setdefault(needle["length"], {}).setdefault(needle["rolling"], []).append(needle)
tops = {length: pow(base, length - 1, M) for length in groups}

def scan(data):
    found = set(label for label, value in literal if value and value in data)
    for length, targets in groups.items():
        if len(data) < length:
            continue
        value = 0
        for byte in data[:length]:
            value = (value * base + byte) % M
        top, last = tops[length], len(data) - length
        index = 0
        while True:
            if value in targets:
                digest = hashlib.sha256(salt + data[index:index + length]).hexdigest()
                found.update(needle["label"] for needle in targets[value] if needle["digest"] == digest)
            if index == last:
                break
            value = ((value - data[index] * top) * base + data[index + length]) % M
            index += 1
    return found

hits, files, total = [], 0, 0
def check(place, data):
    hits.extend([place, label] for label in sorted(scan(data)))
me = str(os.getpid())
try:
    pids = sorted(entry for entry in os.listdir(spec["proc"]) if entry.isdigit() and entry != me)
except OSError:
    pids = []
for pid in pids:
    for part in ("environ", "cmdline", "maps"):
        place = os.path.join(spec["proc"], pid, part)
        try:
            with open(place, "rb") as source:
                data = source.read(LIMIT)
        except OSError:
            continue
        check(place, data)
for root in spec["roots"]:
    for directory, names, entries in os.walk(root, followlinks=False):
        for name in entries:
            place = os.path.join(directory, name)
            if os.path.islink(place):
                continue
            try:
                with open(place, "rb") as source:
                    data = source.read(LIMIT)
            except OSError:
                continue
            files += 1
            total += len(data)
            check(place, data)
print(MARK + json.dumps({"hits": hits, "files": files, "bytes": total}), flush=True)
'''


def program(specification):
    """The guest's Python source, with the specification as its one input."""
    return "MARK = {!r}\nSPEC = {!r}\n".format(MARK, json.dumps(specification)) + PROGRAM


def shell(labelled):
    """The scripted turn's command: plant a fresh control, then run the search for the (label, value) pairs."""
    control = control_value()
    return plant(control) + "python3 - <<'SEARCH'\n" + program(spec(labelled, control=control)) + "SEARCH\n"


def report(output):
    """The guest's one report line among the tool's output."""
    for line in output.splitlines():
        if line.startswith(MARK):
            return json.loads(line[len(MARK):])
    raise AssertionError("the guest search printed no report: " + output[-300:])


def problems(found):
    """Both controls present and no form of the credential anywhere; each problem named."""
    hits = [tuple(hit) for hit in found["hits"]]
    listed = []
    if not any(place.endswith("/run/tqa/token") and label == "control:token" for place, label in hits):
        listed.append("the search did not read /run/tqa/token")
    if not any(place.endswith("/environ") and label == "control:gateway" for place, label in hits):
        listed.append("the search found no runtime environment in /proc")
    if not any(place.endswith("/hashed-control") and label == "control:hashed" for place, label in hits):
        listed.append("the hash matcher did not find the planted control")
    listed += ["the credential is in the guest: {} ({})".format(place, label)
               for place, label in hits if not label.startswith("control:")]
    return listed

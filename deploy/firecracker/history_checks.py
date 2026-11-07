"""SEC-017 on the microVM (contract.py `history`): what the second conversation's guest and VM host may show when two
conversations share one workspace. The state is a block image per conversation (runtime-state/<id>.ext4); the second
guest must hold exactly one state device, its own, and nothing that names the first conversation. A forged history
written into the shared workspace must not change the first conversation's stored events."""
import json

PROBE_MARK = "SEC017-PROBE "
FORGED = "FORGED-HISTORY-MARKER"

# Runs in the guest through the shell tool; one marked JSON line. The walk skips the kernel's own trees.
PROBE = r"""
import json, os, re
state_dir = os.environ.get("TQA_STATE_DIR", "/run/tqa/state")
try:
    state = sorted(os.listdir(state_dir))
except OSError as error:
    state = ["<unreadable: %s>" % type(error).__name__]
mounts = [line.split()[:2] for line in open("/proc/mounts") if line.split()[1] == state_dir]
uuid = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
ids, samples = set(), []
for top, dirs, files in os.walk("/"):
    dirs[:] = [d for d in dirs if os.path.join(top, d) not in ("/proc", "/sys", "/dev")]
    for name in dirs + files:
        for found in uuid.findall(name):
            if found not in ids:
                samples.append(os.path.join(top, name)[:160])
            ids.add(found)
forged = []
frame = '{"type":"event","seq":999999,"event":{"type":"text.delta","text":"__FORGED__"}}\n'
for path in ("/workspace/events.jsonl", "/workspace/.tqa-forged/events.jsonl"):
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "a") as stream:
            stream.write(frame)
        forged.append(path)
    except OSError:
        pass
# Distinct ids and one sample path each keep the record under the tool result cap.
print("__MARK__" + json.dumps({"state": state[:20], "state_mounts": mounts, "ids": sorted(ids)[:50],
                                "samples": samples[:20], "forged": forged}))
""".replace("__FORGED__", FORGED).replace("__MARK__", PROBE_MARK)


def _record(output):
    for line in output.splitlines():
        if line.startswith(PROBE_MARK):
            return json.loads(line[len(PROBE_MARK):])
    return None


def guest_problems(output, first, second):
    """Inside the second guest: one state device, no entry or path naming the first conversation."""
    record = _record(output)
    if record is None:
        return ["no probe record in the second guest's shell output"]
    problems = []
    if len(record["state_mounts"]) != 1:
        problems.append("{} devices are mounted at the state path".format(len(record["state_mounts"])))
    problems += ["the state directory holds the first conversation's {}".format(entry)
                 for entry in record["state"] if first in entry]
    if first in record["ids"]:
        shown = [path for path in record["samples"] if first in path]
        problems.append("the second guest reaches a path naming the first conversation: {}".format(
            shown[0] if shown else first))
    return problems


def host_problems(mounts, first, second):
    """The second VM host binds its own state image at /vm/state.img and nothing naming the first conversation."""
    problems = []
    state = [mount for mount in mounts if mount.get("Destination") == "/vm/state.img"]
    if len(state) != 1:
        problems.append("{} state images are bound in the VM host".format(len(state)))
    elif second not in state[0].get("Source", ""):
        problems.append("the bound state image {} is not the second conversation's".format(state[0].get("Source")))
    problems += ["the VM host binds {} at {}, which names the first conversation".format(
        mount.get("Source"), mount.get("Destination")) for mount in mounts if first in mount.get("Source", "")]
    return problems


def marker_frames(frames):
    """Every frame carrying the forged marker anywhere, tool calls included, for the failure's dump."""
    found = []
    for frame in frames:
        text = json.dumps(frame)
        if FORGED in text:
            found.append({"seq": frame.get("seq"), "type": frame.get("event", {}).get("type", frame.get("type")),
                          "at": text.index(FORGED), "frame": text[:300]})
    return found


def history_problems(before, after):
    """The first conversation's stored events after its resume: the earlier ones unchanged, and no frame after them
    carrying the forged marker. A tool call's arguments are the model's script, not history content, so they are not
    what this checks (the first conversation's own script never holds the marker; the dump shows any frame that does)."""
    problems = []
    if after[:len(before)] != before:
        problems.append("the first conversation's earlier history was rewritten")
    later = [frame for frame in after[len(before):] if frame.get("event", {}).get("type") != "tool.call"]
    if any(FORGED in json.dumps(frame) for frame in later):
        problems.append("a forged frame reached the first conversation's history")
    return problems

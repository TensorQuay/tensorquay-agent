"""H8: the soak with a real local model (R4, SOAK-001's "once with a real local model").

The model is a chat-completions endpoint the operator starts on a GPU reserved for the run, and names with
TQA_SOAK_MODEL_URL (reachable from the gateway container) and TQA_SOAK_MODEL_NAME. The run is 4 h at 4 conversations
with reconnect faults and an hourly gateway restart; a GPU reservation is not where the engine and the database are
also stressed. Each message is a small task; progress counts completed model responses, since a real
model carries no script markers.
"""
import re
from urllib.parse import urlsplit

from soak_audit import require

MIN_CONTEXT = 8192


def route(url, model, context):
    """models.toml's `local` route pointed at the real endpoint (the key file's placeholder bearer is accepted)."""
    parts = urlsplit(url)
    require(parts.scheme in ("http", "https") and parts.hostname and not parts.username and not parts.password
            and re.fullmatch(r"[A-Za-z0-9._:/\-]+", url), "the model endpoint must be a plain http(s) URL")
    require(bool(re.fullmatch(r"[A-Za-z0-9._\-/:]{1,200}", model)), "unsafe model name")
    require(int(context) >= MIN_CONTEXT, "the model's context window must be at least {}".format(MIN_CONTEXT))
    return ("[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='{}'\nauth={{kind='bearer'}}\n"
            "key_ref='secrets/local'\ncontext_window={}\nstream_usage=true\n").format(url, model, int(context))


def prompt(index):
    """One small task: a shell command and a read; every call runs (ADR-0035)."""
    lines = ["Soak task {}. Use the tools, do not describe them.".format(index)]
    lines += ["Run this with the shell tool: printf soak-{0} > soak.txt; sleep 20; cat soak.txt".format(index),
              "Then read soak.txt with the read_file tool, and answer with one line: DONE {}.".format(index)]
    return "\n".join(lines)


class RealProgress:
    """Completed model responses; a real model has no script markers to check."""

    def __init__(self):
        self.completed = 0

    def record(self, event):
        if event.get("type") == "model.completed":
            self.completed += 1


def check_mode(model, hours, rehearsal, conversations):
    """R4 is 4 h at 4 conversations; the fake-model runs are 2, 8 or 24 h."""
    if model == "real":
        require(rehearsal or hours == 4, "the real-model soak runs 4 h (R4)")
        require(conversations == 4, "the real-model soak runs 4 conversations (the GPU bounds it)")
    else:
        require(hours in (2, 8, 24), "--hours must be 2, 8 or 24 with the fake model")

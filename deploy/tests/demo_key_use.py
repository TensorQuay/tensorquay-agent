"""WEB-E2E-029 (model call): the stack's read-audit table is made unwritable, so no key may be used; the model call
fails as the gateway's own key_use_unrecorded with plain words, and the fake model receives no request."""

import json
from pathlib import Path

import compose

BREAK = "ALTER TABLE secret_reads RENAME TO secret_reads_away"
MEND = "ALTER TABLE secret_reads_away RENAME TO secret_reads"
REASON = "gateway.key_use_unrecorded 503"
# The run's code for a failed model call; the reason says it was the gateway's own refusal.
CODE = "provider_error"


def psql(configuration, env, statement):
    compose.command(configuration + ["exec", "-T", "postgres", "psql", "-v", "ON_ERROR_STOP=1", "-U", "tqa",
                                     "-d", "gateway", "-c", statement], env, capture=True)


def model_requests(configuration, env):
    """The fake model's request log lines (one JSON object per request, flushed before it answers)."""
    logs = compose.command(configuration + ["logs", "--no-color", "--no-log-prefix", "fake-model"], env,
                           capture=True).stdout
    return requests_in(logs)


def requests_in(logs):
    count = 0
    for line in logs.splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict) and entry.get("method") == "POST":
            count += 1
    return count


def break_audit(configuration, env):
    """Counts the model's requests so far, then makes every key read's audit row unwritable."""
    env["TQA_KEY_USE_BEFORE"] = str(model_requests(configuration, env))
    psql(configuration, env, BREAK)
    env["TQA_KEY_USE_BROKEN"] = "1"


def mend_audit(configuration, env):
    """Counts the model's requests while the table was broken, then puts the table back for the stack's own checks."""
    if env.pop("TQA_KEY_USE_BROKEN", None) != "1":
        return
    env["TQA_KEY_USE_AFTER"] = str(model_requests(configuration, env))
    psql(configuration, env, MEND)


def verify_result(env):
    result = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())
    reached = int(env["TQA_KEY_USE_AFTER"]) - int(env["TQA_KEY_USE_BEFORE"])
    expected = {"code": CODE, "reason": REASON, "providerMessage": None, "rawShown": []}
    seen = {key: result.get(key) for key in expected}
    if seen != expected or reached != 0:
        raise AssertionError("Unexpected key-use result: {} (model requests while broken: {})".format(result, reached))
    print("Key use (model call): with the read-audit table unwritable the run failed as {} ({}) with plain words, no "
          "provider message and no raw code on the page; the model server received no request".format(
              result["code"], REASON), flush=True)

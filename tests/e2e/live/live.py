"""E2E-PROVIDER-014's live mode: the parts that never touch the network, unit-tested with synthetic values (TK-U-017).

The repository names each route's protocol and settings (providers.ROUTES). The owner's private routes file names what
the repository must not: each route's model, base URL and the env-file variable holding its key. Keys reach only the
gateway's seed; every line the suite prints or saves is checked for key values and base URLs first.
"""
import datetime
import json
import os
from pathlib import Path
import re
import stat
import tomllib

import seed

AUTH_HEADER = re.compile(r"[A-Za-z][A-Za-z0-9-]{0,63}")
MAX_ROUTES_BYTES = 16384


class LiveError(Exception):
    """A refusal; its text never contains a value."""


def private_routes(path):
    """The private routes file: this user's, mode 0600, TOML with one [[routes]] table per suite route."""
    try:
        info = os.stat(path, follow_symlinks=False)
    except OSError:
        raise LiveError("the routes file does not exist") from None
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise LiveError("the routes file must be this user's regular file, mode 0600")
    if info.st_size > MAX_ROUTES_BYTES:
        raise LiveError("the routes file exceeds 16 KiB")
    try:
        return tomllib.loads(Path(path).read_text())
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        raise LiveError("the routes file is not valid TOML") from None


def resolve(document, suite, values):
    """Each suite route's live fields, in suite order: model, base URL (given, or from an env-file variable), the key's
    variable and the auth. Every suite route must be named exactly once, and nothing else."""
    entries = document.get("routes")
    if not isinstance(entries, list) or set(document) != {"routes"}:
        raise LiveError("the routes file holds only [[routes]] tables")
    by_name = {}
    for entry in entries:
        name = entry.get("name") if isinstance(entry, dict) else None
        if name not in {route["name"] for route in suite}:
            raise LiveError("the routes file names a route the suite does not have")
        if name in by_name:
            raise LiveError("the routes file names {} twice".format(name))
        allowed = {"name", "model", "base_url", "base_url_env", "key_env", "auth_header"}
        if not set(entry) <= allowed:
            raise LiveError("route {} has a field other than {}".format(name, ", ".join(sorted(allowed))))
        by_name[name] = entry
    resolved = []
    for route in suite:
        entry = by_name.get(route["name"])
        if entry is None:
            raise LiveError("the routes file has no entry for " + route["name"])
        model, key_env = entry.get("model"), entry.get("key_env")
        if not isinstance(model, str) or not model or not isinstance(key_env, str) or not seed.VARIABLE.fullmatch(key_env):
            raise LiveError("route {} needs a model and a key_env variable".format(route["name"]))
        if ("base_url" in entry) == ("base_url_env" in entry):
            raise LiveError("route {} needs exactly one of base_url and base_url_env".format(route["name"]))
        base_url = entry.get("base_url") or values.get(entry["base_url_env"], "")
        if not base_url.startswith("https://") or any(c in base_url for c in "'\"\\\n "):
            raise LiveError("route {}'s base URL must be an https URL".format(route["name"]))
        header = entry.get("auth_header")
        if header is not None and not (isinstance(header, str) and AUTH_HEADER.fullmatch(header)):
            raise LiveError("route {}'s auth_header is not a header name".format(route["name"]))
        if any(c in model for c in "'\"\\\n"):
            raise LiveError("route {}'s model has a quote or a backslash".format(route["name"]))
        resolved.append(dict(route, model=model, base_url=base_url.rstrip("/"), key_env=key_env, auth_header=header))
    return resolved


def models_toml(routes, context_window):
    """The live routes, each keyed on its own seeded secret."""
    blocks = []
    for route in routes:
        auth = ("{{kind='header',name='{}'}}".format(route["auth_header"]) if route["auth_header"]
                else "{kind='bearer'}")
        blocks.append(
            "[[routes]]\nname='{name}'\nprotocol='{protocol}'\nbase_url='{base_url}'\nmodel='{model}'\n"
            "auth={auth}\nkey_ref='secrets/live-{name}'\nnative_tools=true\nstream_usage=true\n"
            "context_window={window}\n{settings}".format(auth=auth, window=context_window, **route))
    return "".join(blocks)


class Guard:
    """Refuses any text that contains a key value or a base URL; everything the live suite shows passes through it."""

    def __init__(self, routes, values):
        self.secrets = [values[route["key_env"]] for route in routes if values.get(route["key_env"])]
        self.secrets += [route["base_url"] for route in routes]

    def check(self, text):
        if any(secret in text for secret in self.secrets):
            raise LiveError("a key value or a base URL reached the output; nothing was shown")
        return text


def summary(rows):
    """The result table: one row per route, tokens and requests, no model names or URLs."""
    lines = ["| Route | Protocol | Settings | Probe | Task | Tokens (prompt / completion) | Requests |",
             "|---|---|---|---|---|---|---|"]
    for row in rows:
        lines.append("| {route} | {protocol} | {settings} | {probe} | {task} | {prompt} / {completion} | {requests} |"
                     .format(**row))
    return "\n".join(lines) + "\n"


def settings_label(route):
    """A route's settings in one short cell."""
    text = route["settings"].replace("\n", " ").strip()
    return json.dumps(text)[1:-1] if text else "defaults"


def attempt_totals(attempts):
    """Tokens and requests over tqa-e2e attempt records, whose tally fields sit at the top level of each record."""
    return (sum(attempt.get("prompt_tokens", 0) for attempt in attempts),
            sum(attempt.get("completion_tokens", 0) for attempt in attempts),
            sum(attempt.get("steps", 0) for attempt in attempts))


def stamp(label, project, now=None):
    """One timeline line, `HH:MM:SS.mmm <label> <project>`, so a stack's life can be matched to the daemon's logs."""
    now = now or datetime.datetime.now().astimezone()
    return "{} {} {}".format(now.strftime("%H:%M:%S.%f")[:12], label, project or "-")


def attempt_details(attempts, bound=500):
    """Each failed attempt's class and detail, for the private report; bounded, and shown only through the guard."""
    return ["{}: {}".format(attempt.get("class"), attempt["detail"][:bound])
            for attempt in attempts if attempt.get("detail")]


def seed_removal(directory, toybox):
    """The command that removes the installation's secret seed. The gateway reads the seed once at start-up, so the keys
    leave the disk as soon as the stack is up, before any paid call and whatever happens at cleanup."""
    return ["docker", "run", "--rm", "--pull", "never", "--network", "none", "--mount",
            "type=bind,source={},target=/install".format(directory), toybox, "rm", "-f", "/install/secrets.toml"]


def seed_absent(directory, toybox):
    """The command that succeeds only when the seed is gone; the installation belongs to the container user, so the host
    can't look inside it."""
    return ["docker", "run", "--rm", "--pull", "never", "--network", "none", "--mount",
            "type=bind,source={},target=/install".format(directory), toybox, "test", "!", "-e", "/install/secrets.toml"]

"""The wave stack profile (TEST-PLAN E2E-WAVE-002): make a stack admit N attempts at once for a wave, and put it back.

Admission is two runtime-mutable limits (ADR-0028): `admission.slots` and `admission.per_user`, each at most the
configuration file's `sandbox.max_live`, with `per_user` at most `slots`. This tool reads them through the admin limits
API with an administrator's token, raises what is short of N (slots first), and records the values it found in the run
directory so `restore` puts back exactly those (per_user first). A `sandbox.max_live` below N can't be changed at run
time: the tool says so and changes nothing.

The model server's own concurrency (its sequence limit) is set where the server starts and is recorded with the run.

Usage:
  wave_profile.py check   --base-url URL --token-file FILE --concurrency N
  wave_profile.py apply   --base-url URL --token-file FILE --concurrency N --record RUN_DIR
  wave_profile.py restore --base-url URL --token-file FILE --record RUN_DIR

The token file must be mode 0600; the token is never printed.
"""

import argparse
import json
import secrets
import stat
import sys
import urllib.error
import urllib.request
from pathlib import Path

SLOTS, PER_USER = "admission.slots", "admission.per_user"
RECORD = "wave-profile-before.json"


def _items(listing):
    return {item["key"]: item for item in listing}


def plan(listing, concurrency):
    """The writes that make the stack admit `concurrency` attempts at once, in order, or a problem that no write can
    fix. `listing` is the admin limits API's `items`."""
    items = _items(listing)
    for key in (SLOTS, PER_USER):
        if key not in items:
            return [], "the gateway lists no {} limit; it needs admission support".format(key)
    slots, per_user = items[SLOTS], items[PER_USER]
    if slots["max"] < concurrency:
        return [], ("the gateway's sandbox.max_live allows {} sandboxes at once and the wave needs {}: raise "
                    "[sandbox] max_live in the gateway's configuration file and restart it".format(slots["max"],
                                                                                                   concurrency))
    writes = []
    if slots["value"] < concurrency:
        writes.append((SLOTS, concurrency))
    if per_user["value"] < concurrency:
        writes.append((PER_USER, concurrency))
    return writes, None


def restore_plan(before, listing):
    """The writes that put back the recorded values, lowering `per_user` before `slots` so the order rule holds at
    every step."""
    items = _items(listing)
    return [(key, before[key]) for key in (PER_USER, SLOTS) if items[key]["value"] != before[key]]


class Api:
    def __init__(self, base_url, token):
        self.base_url = base_url.rstrip("/")
        self._token = token

    def call(self, method, path, body=None):
        headers = {"Authorization": "Bearer " + self._token, "Content-Type": "application/json"}
        if method != "GET":
            headers["Idempotency-Key"] = secrets.token_hex(16)
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base_url + path, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as error:
            try:
                code = json.loads(error.read()).get("error", {}).get("code", "")
            except ValueError:
                code = ""
            raise SystemExit("{} {} answered {} {}".format(method, path, error.code, code).rstrip()) from None
        except urllib.error.URLError as error:
            raise SystemExit("{} {} failed: {}".format(method, path, error.reason)) from None

    def listing(self):
        return self.call("GET", "/api/v1/admin/limits")["items"]

    def write(self, key, value):
        # Each write names the version it read, so a concurrent change is refused rather than overwritten.
        version = _items(self.listing())[key]["version"]
        self.call("PUT", "/api/v1/admin/limits/" + key, {"value": value, "version": version})
        print("set {} = {}".format(key, value))


def read_token(path):
    path = Path(path)
    mode = stat.S_IMODE(path.stat().st_mode)
    if mode & 0o077:
        raise SystemExit("the token file must be mode 0600 (it is {:04o}): {}".format(mode, path))
    return path.read_text().strip()


def admitted(listing):
    items = _items(listing)
    return "slots {}, per_user {} (sandbox.max_live {})".format(items[SLOTS]["value"], items[PER_USER]["value"],
                                                               items[SLOTS]["max"])


def check(api, concurrency):
    listing = api.listing()
    writes, problem = plan(listing, concurrency)
    print("admission now: " + admitted(listing))
    if problem:
        print("cannot run {} at once: {}".format(concurrency, problem))
        return 1
    if writes:
        print("apply would set: " + ", ".join("{} = {}".format(key, value) for key, value in writes))
        return 1
    print("the stack admits {} attempts at once".format(concurrency))
    return 0


def apply(api, concurrency, record):
    target = Path(record) / RECORD
    if target.exists():
        print("{} already exists: restore from it first; it is never overwritten".format(target))
        return 1
    listing = api.listing()
    writes, problem = plan(listing, concurrency)
    if problem:
        print("cannot run {} at once: {}".format(concurrency, problem))
        return 1
    items = _items(listing)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps({"concurrency": concurrency,
                                  "limits": {key: items[key]["value"] for key in (SLOTS, PER_USER)}}, indent=1) + "\n")
    for key, value in writes:
        api.write(key, value)
    print("admission now: " + admitted(api.listing()))
    return 0


def restore(api, record):
    target = Path(record) / RECORD
    before = json.loads(target.read_text())["limits"]
    for key, value in restore_plan(before, api.listing()):
        api.write(key, value)
    print("admission now: " + admitted(api.listing()))
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("check", "apply", "restore"))
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--concurrency", type=int)
    parser.add_argument("--record", help="the run directory that keeps the values found before apply")
    args = parser.parse_args(argv)
    if args.action != "restore" and (args.concurrency is None or args.concurrency < 1):
        parser.error("--concurrency N (at least 1) is required for " + args.action)
    if args.action != "check" and not args.record:
        parser.error("--record RUN_DIR is required for " + args.action)
    try:
        api = Api(args.base_url, read_token(args.token_file))
        if args.action == "check":
            return check(api, args.concurrency)
        if args.action == "apply":
            return apply(api, args.concurrency, args.record)
        return restore(api, args.record)
    except SystemExit as stop:
        if isinstance(stop.code, str):
            print(stop.code)
            return 1
        raise


if __name__ == "__main__":
    sys.exit(main())

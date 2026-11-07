"""E2E-PROVIDER-014: the live suite's gateway secret seed, from the owner's private env file (TK-U-017).

Each route's key goes only into the gateway's secret store, through the seed file the installation mounts
(TQA_SECRETS_SEED_FILE): one `secrets/live-<route>` entry per route, in a mode 0600 file inside a mode 0700 directory.
No value is ever printed, logged or put in an error; errors name the variable or the problem instead.

Run: python3 tests/e2e/live/seed.py --env-file <private env file> --out-dir <new directory> --route <route>=<VARIABLE>...
It prints only the seed file's path.
"""
import argparse
import json
import os
from pathlib import Path
import re
import stat
import sys

NAME = re.compile(r"[a-z0-9][a-z0-9-]{0,62}")
VARIABLE = re.compile(r"[A-Z_][A-Z0-9_]{0,127}")
MAX_ENV_BYTES = 65536


class SeedError(Exception):
    """A refusal; its text never contains a value."""


def private_file(path):
    """The file's text, when it is a regular file of this user that no one else may read or write."""
    try:
        info = os.stat(path, follow_symlinks=False)
    except OSError:
        raise SeedError("the env file does not exist") from None
    if not stat.S_ISREG(info.st_mode):
        raise SeedError("the env file is not a regular file")
    if info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise SeedError("the env file must be this user's and mode 0600")
    if info.st_size > MAX_ENV_BYTES:
        raise SeedError("the env file exceeds 64 KiB")
    try:
        return Path(path).read_text()
    except (OSError, UnicodeDecodeError):
        raise SeedError("the env file cannot be read as text") from None


def parse(text):
    """`NAME=value` lines; blank lines, comments and an `export ` prefix are allowed; one pair of quotes is removed."""
    values = {}
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, separator, value = line.removeprefix("export ").partition("=")
        name = name.strip()
        if not separator or not VARIABLE.fullmatch(name):
            raise SeedError("line {} of the env file is not NAME=value".format(number))
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
            value = value[1:-1]
        values[name] = value
    return values


def seed_text(routes, values):
    """The seed TOML: one entry per route, from the variable it names."""
    lines = ["[secrets]"]
    for route, variable in routes:
        value = values.get(variable)
        if not value:
            raise SeedError("the env file has no value for " + variable)
        if any(ord(character) < 0x20 or ord(character) == 0x7f for character in value):
            raise SeedError("the value of {} contains a control character".format(variable))
        # A JSON string of printable text is a valid TOML basic string.
        lines.append("{} = {}".format(json.dumps("secrets/live-" + route), json.dumps(value)))
    return "\n".join(lines) + "\n"


def write_seed(env_file, routes, out_dir):
    """Writes the seed into `out_dir` (created mode 0700; it must not exist) and returns the seed file's path."""
    if not routes:
        raise SeedError("at least one route is required")
    for route, variable in routes:
        if not NAME.fullmatch(route) or not VARIABLE.fullmatch(variable):
            raise SeedError("a route is <name>=<VARIABLE>: lowercase name, uppercase variable")
    if len({route for route, _ in routes}) != len(routes):
        raise SeedError("a route is named twice")
    text = seed_text(routes, parse(private_file(env_file)))
    out_dir = Path(out_dir)
    try:
        out_dir.mkdir(mode=0o700)
    except OSError:
        raise SeedError("the output directory must be new") from None
    path = out_dir / "secrets.toml"
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w") as file:
        file.write(text)
    return path


def route_pair(text):
    route, separator, variable = text.partition("=")
    if not separator:
        raise argparse.ArgumentTypeError("a route is <name>=<VARIABLE>")
    return route, variable


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--env-file", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--route", action="append", default=[], type=route_pair)
    args = parser.parse_args(argv)
    try:
        print(write_seed(args.env_file, args.route, args.out_dir))
    except SeedError as error:
        print("seed: " + str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())

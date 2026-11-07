"""The release pipeline's refusals (REL-U-004): everything that stops a release before anything is built. Each function
judges git's own output, so it is tested without a repository."""

from pathlib import Path
import re

VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-rc\.[1-9]\d*)?")
FULL_SHA = re.compile(r"[0-9a-f]{40}")


class Refused(Exception):
    """A release that must not start; the message says why."""


def version(text):
    """`X.Y.Z` or `X.Y.Z-rc.N`; no `v` prefix, no leading zeros, no other suffix."""
    if not VERSION.fullmatch(text):
        raise Refused("the version must be X.Y.Z or X.Y.Z-rc.N (got {!r})".format(text))
    return text


def clean_checkout(porcelain):
    """The checkout running the pipeline is its committed code: `git status --porcelain` is empty."""
    changed = [line[3:] for line in porcelain.splitlines() if line.strip()]
    if changed:
        raise Refused("the checkout has uncommitted changes: " + ", ".join(changed[:10]))


def commit(rev_parse):
    """The one full SHA `git rev-parse --verify <commit>^{commit}` printed."""
    lines = [line.strip() for line in rev_parse.splitlines() if line.strip()]
    if len(lines) != 1 or not FULL_SHA.fullmatch(lines[0]):
        raise Refused("the commit does not resolve to one commit")
    return lines[0]


def tag(release_version, sha, rev_parse):
    """`vX.Y.Z`: absent (the owner creates it), on this commit, or moved (refused). `rev_parse` is
    `git rev-parse -q --verify refs/tags/vX.Y.Z^{commit}`, empty when the tag is absent."""
    name = "v" + release_version
    tagged = rev_parse.strip()
    if not tagged:
        return {"tag": name, "state": "to create"}
    if tagged != sha:
        raise Refused("the tag {} names {}, not {} (a moved tag)".format(name, tagged[:12], sha[:12]))
    return {"tag": name, "state": "exists"}


def releasable(on_main, candidate):
    """A release comes from main; a candidate dry run is never releasable, whatever its commit."""
    if candidate:
        return False
    if not on_main:
        raise Refused("the commit is not on main; use --candidate for a dry run")
    return True


def release_directory(out, release_version):
    """`out/X.Y.Z`, which must not exist, nor a partial one from an earlier attempt."""
    final, partial = Path(out) / release_version, Path(out) / ".{}.partial".format(release_version)
    if final.exists():
        raise Refused("{} exists; a release directory is never overwritten".format(final))
    if partial.exists():
        raise Refused("{} exists from an earlier attempt; read its FAILED file, then remove it".format(partial))
    return final

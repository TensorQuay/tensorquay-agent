"""Owned canaries and shell writes inside the actual runtime's mount namespace."""
import errno
import hashlib
import json
from pathlib import Path
import sys


def digests():
    return {name: hashlib.sha256(Path(name).read_bytes()).hexdigest()
            for name in ["/etc/passwd", "/run/tqa/token"]}


def setup():
    Path("/tmp/outside-canary").write_text("outside-read-marker")
    Path("/workspace/escape").symlink_to("/tmp/outside-canary")
    Path("/workspace/dir").mkdir()
    Path("/workspace/.git").mkdir()
    Path("/workspace/.git/config").write_text("git-config-before")
    Path("/workspace/inside").write_text("workspace-positive")
    Path("/tmp/absolute-positive").write_text("temporary-positive")
    Path("/run/tqa/state/escape-positive").write_text("state-positive")
    Path("/tmp/escape-baseline.json").write_text(json.dumps(digests()))
    print("absolute workspace, tmp and own-state writes succeeded")


def verify():
    assert Path("/tmp/outside-canary").read_text() == "outside-read-marker"
    # The file tool's write inside .git landed (ADR-0035); confinement.GIT_CONFIG.
    assert Path("/workspace/.git/config").read_text() == "git-config-by-file-tool"
    assert Path("/workspace/inside").read_text() == "workspace-positive"
    assert Path("/tmp/absolute-positive").read_text() == "temporary-positive"
    assert Path("/run/tqa/state/escape-positive").read_text() == "state-positive"
    assert not Path("/x").exists(), "traversal created a file outside the workspace"
    for path in ["/root-probe", "/usr/escape-probe", "/etc/escape-probe", "/run/tqa/token"]:
        try:
            Path(path).write_text("escape-attempt")
        except OSError as error:
            assert error.errno in (errno.EROFS, errno.EACCES), "unexpected refusal"
        else:
            raise AssertionError("shell escaped the filesystem boundary")
    for path in ["/root-probe", "/usr/escape-probe", "/etc/escape-probe"]:
        assert not Path(path).exists(), "shell left an outside file"
    assert digests() == json.loads(Path("/tmp/escape-baseline.json").read_text())
    print("outside canaries unchanged; root and token writes refused")


{"setup": setup, "verify": verify}[sys.argv[1]]()

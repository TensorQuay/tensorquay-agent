"""WEB-E2E-071..077: the Desktop environment end to end on the scripted model (the computer-use stack suite).

The browser spec (web/e2e/desktop-stack.spec.ts) drives Desktop tasks through fixtures/desktop.json: each case's phrase
picks its replies. The model's desktop actions are keys, typing, waits and `observe` only, as SEC-057's are, since a
click needs a ref from a snapshot a script can't know. The Desktop built-in is the image `TQA_DESKTOP_TEST_IMAGE` names
(`compose.builtin_pins` pins it), never the engine's bare tag. WEB-E2E-073's gateway restart goes through the same
markers as the agents suite (`demo_agents.Restarter`). WEB-E2E-071's pictures, stored under its task as PNGs, are
checked on disk while that task still exists (`PictureWatch`), since the spec's afterEach deletes every task, and its
pictures with it.
"""

import json
import os
import tempfile
import threading
import time
import uuid
from pathlib import Path

import compose
import demo_agents

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"
PNG = b"\x89PNG\r\n\x1a\n"
# WEB-E2E-071 writes WANTED (its task's id) in the markers folder; PictureWatch answers with CHECKED (the pictures'
# number) or FAILED (why).
WANTED, CHECKED, FAILED = "pictures-wanted", "pictures-checked", "pictures-failed"


def configure(directory, env):
    """The suite needs the Desktop under test: a run that names no image is refused, never given the engine's tag."""
    if not env.get("TQA_DESKTOP_TEST_IMAGE"):
        raise ValueError("The desktop suite needs TQA_DESKTOP_TEST_IMAGE: the Desktop image under test "
                         "(deploy/build-images.sh --desktop)")
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    text = config.read_text()
    if "desktop = " not in text.split("[runtime.builtin_images]", 1)[-1]:
        raise ValueError("The gateway's file pins no Desktop image")
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared desktop scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/desktop.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "desktop.json").resolve()), "Unexpected scenario source"


def check_pictures(root, identity):
    """The task's pictures under `root`: PNG files in its folder, bounded, none escaping it. Returns their number."""
    root = Path(root).resolve()
    folder = root / identity
    assert root in folder.resolve().parents, "The screenshots escaped the storage root"
    pictures = sorted(folder.glob("*.png"))
    assert pictures, "WEB-E2E-071's task kept no pictures"
    for picture in pictures:
        assert folder.resolve() in picture.resolve().parents, "A picture escaped its task's folder"
        size = picture.stat().st_size
        assert 0 < size <= 8 * 1024 * 1024, "Unexpected picture size"
        with picture.open("rb") as handle:
            assert handle.read(8) == PNG, "A stored picture isn't a PNG"
    return len(pictures)


def copy_pictures(env, identity, destination):
    """The task's pictures folder copied to `destination`/<identity>, read-only from a container and handed to the
    host user: during the run the storage root is the gateway user's (on a rootless daemon, a subordinate id the host
    user can't read). `cp -a` keeps a link as a link, so check_pictures still sees one that points out. A task with no
    folder gets an empty one."""
    storage = Path(env["TQA_STORAGE_ROOT"]).resolve()
    compose.command(["docker", "run", "--rm", "--network", "none",
                     "--mount", f"type=bind,source={storage},target=/storage,readonly",
                     "--mount", f"type=bind,source={destination},target=/out", compose.TOYBOX,
                     "sh", "-c", 'if [ -e "/storage/screens/$1" ]; then cp -a "/storage/screens/$1" "/out/$1"; '
                     'else mkdir "/out/$1"; fi && chown -R "$2" "/out/$1"', "copy", identity,
                     compose.host_owner(env)], env)


class PictureWatch:
    """Checks WEB-E2E-071's pictures while its task exists: each time the spec writes WANTED (the task's id) in
    `markers`, a copy of that task's pictures folder is checked, WANTED removed, and CHECKED (their number) or FAILED
    (why) written, so a retried test asks again. `outcome` is the last answer, ("checked", id, number) or ("failed",
    id or None, why), or None if never asked."""

    def __init__(self, markers, env, copy=copy_pictures):
        self.markers = Path(markers)
        self.env = env
        self.copy = copy
        self.stop = threading.Event()
        self.outcome = None
        self.thread = threading.Thread(target=self.run, name="desktop-pictures", daemon=True)

    def start(self):
        self.markers.mkdir(mode=0o700, exist_ok=True)
        self.thread.start()
        return self

    def run(self):
        # The whole browser run is the bound, as the restarter's; the spec's own short wait fails first.
        deadline = time.monotonic() + 1800 * demo_agents.scale(self.env)
        wanted = self.markers / WANTED
        while True:
            while not wanted.exists():
                if self.stop.is_set() or time.monotonic() > deadline:
                    return
                time.sleep(0.2)
            self.answer(wanted)

    def answer(self, wanted):
        identity = None
        try:
            identity = str(uuid.UUID(wanted.read_text().strip()))
            with tempfile.TemporaryDirectory(prefix="tqa-pictures-") as copied:
                self.copy(self.env, identity, Path(copied))
                number = check_pictures(copied, identity)
        except Exception as error:  # Reported to the spec, which fails naming it, and kept for the verdict.
            why = compose.redact(str(error) or type(error).__name__, self.env)
            self.outcome, name, text = ("failed", identity, why), FAILED, why
        else:
            self.outcome, name, text = ("checked", identity, number), CHECKED, str(number)
        wanted.unlink(missing_ok=True)
        self.mark(name, text)

    def mark(self, name, text):
        """Written whole or not at all, as the restarter's markers."""
        partial = self.markers / (name + ".partial")
        partial.write_text(text)
        os.replace(partial, self.markers / name)

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=60)
        return self.outcome


def verify_result(env, restarter, pictures):
    """WEB-E2E-071's looked-at task had its pictures checked on disk, during the run, and they passed (PictureWatch).
    Then the restarter stops (WEB-E2E-073's spec asserts its restart itself, through the markers)."""
    try:
        outcome = pictures.finish() if pictures is not None else None
        result = Path(env["TQA_STACK_RESULT"])
        assert result.stat().st_size <= 4096, "Desktop result exceeded its bound"
        identity = str(uuid.UUID(json.loads(result.read_text())["lookedAt"]))
        assert outcome is not None, "WEB-E2E-071's pictures were never checked (no {} from the spec)".format(WANTED)
        kind, checked, detail = outcome
        assert checked == identity, "The pictures checked weren't the looked-at task's"
        assert kind == "checked", "WEB-E2E-071's pictures: {}".format(detail)
        print("Desktop: {} pictures kept for the looked-at task".format(detail), flush=True)
    finally:
        if restarter is not None:
            restarter.finish()

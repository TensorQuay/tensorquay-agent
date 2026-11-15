"""WEB-E2E-126: read_file on PDFs on the real stack (PRD item 30): the pages' text for every model, and the pages as
pictures for a model that reads them; a locked, a damaged and an over-nested file saying so in words.

The browser spec (web/e2e/pdf-stack.spec.ts) attaches the files it builds and drives fixtures/pdf.json's tasks on the
stack's two routes: `local`, the base installation's, and `vision`, the same scripted model with `vision=true`, so the
runtime draws the pages the agent read and sends them with the tool's result (CORE-U-092). The stack's uploads are
checked on disk afterwards, byte for byte against what the browser sent.
"""

import hashlib
import json
from pathlib import Path
import uuid

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"
# The route that reads pictures: the base route again, under its own name, with `vision=true` (the gateway's
# `[[routes]]` key; a route without it gets the pages' text only).
VISION_ROUTE = "vision"
# Its name in the picker. Two routes of one model would read alike ("Fixture"), and the picker then numbers them
# ("Fixture (1)"): a label no catalogue entry has, which the shared sign-in's chrome check refuses (ux/chrome.ts).
VISION_DISPLAY_NAME = "Vision fixture"


def configure(directory, env):
    """A second route to the scripted model, `vision`, that reads pictures, under its own picker name."""
    compose.ownership(directory, env)
    models = directory / "models.toml"
    text = models.read_text()
    if "name='local'" not in text:
        raise ValueError("The stack's models file has no local route to copy")
    vision = text.replace("name='local'", "name='" + VISION_ROUTE + "'\ndisplay_name='" + VISION_DISPLAY_NAME + "'", 1)
    compose.write(models, text + "\n" + vision + "vision=true\n")
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared PDF scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/pdf.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "pdf.json").resolve()), "Unexpected scenario source"


def verify_result(env):
    """Every PDF the browser attached is in its task's workspace, byte for byte, and nothing else was written there:
    read_file reads, it never changes a file, and the pages drawn for the vision route stay out of the workspace."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "PDF result exceeded its bound"
    record = json.loads(result.read_text())
    root = Path(env["TQA_STORAGE_ROOT"]).resolve()
    checked = 0
    for entry in record["tasks"]:
        workspace = root / "workspaces" / str(uuid.UUID(entry["workspaceId"]))
        assert workspace.is_dir() and not workspace.is_symlink(), "Missing workspace"
        uploads = workspace / "uploads"
        for name, digest in entry["uploads"].items():
            upload = uploads / name
            assert upload.is_file() and not upload.is_symlink(), "The upload {} is not in the workspace".format(name)
            assert hashlib.sha256(upload.read_bytes()).hexdigest() == digest, "The stored {} differs from what was sent".format(name)
            checked += 1
        written = sorted(str(path.relative_to(workspace)) for path in workspace.rglob("*")
                         if path.is_file() and not str(path.relative_to(workspace)).startswith(".tqa/"))
        assert written == sorted("uploads/" + name for name in entry["uploads"]), \
            "The workspace holds more than the uploads: {}".format(written)
    assert checked == 5, "Expected the five attached PDFs, found {}".format(checked)
    print("PDF: {} uploads stored unchanged; the agent read them and wrote nothing".format(checked), flush=True)

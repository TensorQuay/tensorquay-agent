"""E2E-EXT-015, the Gate 6 demo (DEV-PLAN Phase 6): its fixtures and checks, without a stack.

Runbook (tests/e2e/gate6.py, over the public API on the Docker stack; no browser):
1. The install file registers the `tracker` connector: the remote MCP server of the connector fixture over HTTPS (its
   private CA is file-only by design), header credential `X-Api-Key`, off by default,
   and `secret_ref = secrets/tracker-api-key`. A second connector, `elsewhere`, is registered and never enabled.
2. The administrator sees `tracker` with `key_missing: true`, stores the credential in the secret store
   (`PUT /api/v1/admin/secrets/tracker-api-key`), allows the connector to use it (`PUT .../consumers`), and then sees
   `key_missing: false`. The credential is generated per run.
3. The administrator invites a member. The member has no credential of their own for `tracker`; a task made without
   a profile has no connector.
4. The member creates a personal profile allowing `read_file` and the `tracker` connector, and a task from it.
5. The member asks for an issue to be filed. The model's first reply is held (TK-U-035) while a capture joins the
   sandbox's network; then it calls `mcp.tracker.create_issue`, which runs with nothing asked first (ADR-0035), and the
   run completes.

Expected evidence (every item is asserted; the run fails on the first miss):
- E1 registration: the admin view names `secrets/tracker-api-key` and flips `key_missing` from true to false; the
  secret's consumers are exactly the `tracker` connector.
- E2 the member: no own credential; a task without a profile has no connector; the task from the profile has exactly
  `tracker`, and its profile copy sets the connectors.
- E3 the run: `tracker` ready with one tool; one call, `mcp.tracker.create_issue`, answered
  "Created issue #7"; the run completes (SEC-013 driver's checks, unchanged).
- E4 the remote: exactly one call with the model's arguments and the stored credential, from the gateway's address,
  with no `Tqa-*` header; `elsewhere` is refused inside the sandbox with 403 `connector_not_allowed`.
- E5 absence: the credential, in every encoded form, is absent from the sandbox's reassembled traffic, processes and
  writable files (positive controls find the runtime token and environment), and from the events, every container
  log, the database dump and every API response either person received.
- E6 audit: the secret's read records hold a read by the `tracker` connector for this task, outcome `ok`.
"""

import json
import tomllib

CONNECTOR = "tracker"
SECRET_NAME = "tracker-api-key"
SECRET_REF = "secrets/" + SECRET_NAME
PROFILE_NAME = "issue-filing"
MEMBER_EMAIL = "member@example.invalid"


def seeded(config_text):
    """The connector fixture's gateway.toml with `tracker` off by default and naming the shared secret."""
    if config_text.count("enabled_by_default = true") != 1:
        raise ValueError("the seed must hold exactly one connector enabled by default")
    if config_text.count("\n[connectors.auth]\n") < 1:
        raise ValueError("the seed has no connector credential scheme")
    text = config_text.replace("enabled_by_default = true", "enabled_by_default = false")
    reference = "\nsecret_ref = " + json.dumps(SECRET_REF) + "\n[connectors.auth]\n"
    text = text.replace("\n[connectors.auth]\n", reference, 1)
    connectors = {item["name"]: item for item in tomllib.loads(text).get("connectors", [])}
    tracker = connectors.get(CONNECTOR, {})
    if tracker.get("secret_ref") != SECRET_REF or tracker.get("enabled_by_default") is not False:
        raise ValueError("the tracker connector must name the shared secret and be off by default")
    if any(item.get("secret_ref") for name, item in connectors.items() if name != CONNECTOR):
        raise ValueError("only tracker names the shared secret")
    return text


def profile():
    """The member's personal profile: one built-in tool and the tracker connector, nothing else."""
    return {"name": PROFILE_NAME, "display_name": "Issue filing", "model_route": "local",
            "instructions": "File issues in the tracker.", "tools": ["read_file"], "skills": "none",
            "mcp_servers": [], "connectors": [CONNECTOR]}


def tracker(connectors):
    items = [item for item in connectors["items"] if item["name"] == CONNECTOR]
    assert len(items) == 1, "the admin view must list tracker once: " + json.dumps(connectors)
    return items[0]


def check_registration(before, after, secrets):
    """E1: the admin view before and after the credential is stored, and the secret's consumers."""
    first, second = tracker(before), tracker(after)
    assert first.get("secret_ref") == SECRET_REF and second.get("secret_ref") == SECRET_REF, (first, second)
    assert (first["key_missing"], second["key_missing"]) == (True, False), (first["key_missing"], second["key_missing"])
    # No connector class any more (ADR-0035): the network allow-list is the guardrail.
    assert first["enabled_by_default"] is False and "class" not in first, first
    entries = [item for item in secrets["items"] if item["ref"] in (SECRET_NAME, SECRET_REF)]
    assert len(entries) == 1, "the secret store must list the shared secret once: " + json.dumps(secrets)
    assert entries[0]["consumers"] == [{"kind": "connector", "name": CONNECTOR}], entries[0]
    assert "value" not in entries[0], "the admin view must never show a value"
    return {"secret": entries[0]["ref"], "consumers": entries[0]["consumers"]}


def check_member(own, control, task):
    """E2: no own credential; no connector without the profile; exactly tracker from it."""
    mine = [item for item in own["items"] if item["name"] == CONNECTOR]
    assert len(mine) == 1 and mine[0]["has_credential"] is False, "the member must have no own credential: " + \
        json.dumps(own)
    assert control.get("connectors") == [] and not control.get("profile"), "a task without the profile: " + \
        json.dumps(control)
    copy = task.get("profile") or {}
    assert copy.get("name") == PROFILE_NAME and "connectors" in copy.get("sets", []), json.dumps(copy)
    assert task.get("connectors") == [CONNECTOR], "the task from the profile: " + json.dumps(task)


def check_reads(page, conversation_id):
    """E6: the secret's read records name the tracker connector's read for this task."""
    reads = [item for item in page["items"] if item["consumer_kind"] == "connector" and item["consumer"] == CONNECTOR
             and item["conversation_id"] == conversation_id]
    assert reads and all(item["outcome"] == "ok" for item in reads), "no ok read by tracker: " + json.dumps(page)
    return sum(item["count"] for item in reads)

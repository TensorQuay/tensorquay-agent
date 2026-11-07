-- ADR-0035: per-call permissions, approvals and parking are removed; the network allow-list is the guardrail.

-- A run waiting for an approval can never go on: one parked for it, or one still asking (its last stored frame an
-- unanswered permission.request, inside the park delay). It ends failed, with the next sequence, so every history ends
-- in a terminal event and a message starts a new run. Its frame is the one the runtime would have stored; its code,
-- approvals_removed, lets clients explain it. Only active conversations: an archived or deleted one is never resumed,
-- so its history may still end parked. A request stored as a blob (it held a NUL) is not recognised, and its run is
-- left as it is.
WITH latest AS (
    SELECT DISTINCT ON (e.conversation_id) e.conversation_id, e.lifecycle,
           COALESCE(e.run_id, e.frame->>'run_id', e.frame->'event'->>'run_id') AS run_id
    FROM events e JOIN conversations c ON c.id = e.conversation_id
    WHERE e.lifecycle IS NOT NULL AND c.state = 'active'
    ORDER BY e.conversation_id, e.seq DESC
), last AS (
    SELECT DISTINCT ON (e.conversation_id) e.conversation_id, e.seq, e.frame->>'type' AS kind
    FROM events e JOIN latest l ON l.conversation_id = e.conversation_id
    ORDER BY e.conversation_id, e.seq DESC
), ended AS (
    SELECT l.conversation_id, l.run_id, t.seq + 1 AS seq,
           to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS ts
    FROM latest l JOIN last t ON t.conversation_id = l.conversation_id
    WHERE l.run_id IS NOT NULL AND (l.lifecycle = 'run.parked'
        OR (l.lifecycle IN ('run.started', 'run.resumed') AND t.kind = 'permission.request'))
)
INSERT INTO events (conversation_id, seq, run_id, step, ts, frame, lifecycle)
SELECT conversation_id, seq, run_id, NULL, CURRENT_TIMESTAMP,
       jsonb_build_object('type', 'event', 'seq', seq, 'run_id', run_id, 'step', NULL, 'ts', ts,
           'event', jsonb_build_object('type', 'run.failed', 'run_id', run_id,
               'reason', 'approvals were removed; send a message to continue', 'code', 'approvals_removed')),
       'run.failed'
FROM ended;

-- Approvals, session grants and the proxy's allow-once uses (0021, 0022, 0045).
DROP TABLE permission_bindings;
-- The connector tool hints only tightened a connector's permission class (0045).
DROP TABLE connector_tool_hints;

-- A shared profile's policy and rules go, audited like a seed change; personal profiles are their owners' data and
-- are not audited, and conversations' profile copies are rewritten silently.
WITH changed AS (
    UPDATE agent_profiles SET document = document - 'policy' - 'rules', version = version + 1, updated_at = CURRENT_TIMESTAMP
    WHERE document ? 'policy' OR document ? 'rules'
    RETURNING owner_id, document->>'name' AS name, version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'profiles', name, 'replace', version - 1, version, true
FROM changed WHERE owner_id IS NULL;
UPDATE conversations SET profile = profile - 'policy' - 'rules' WHERE profile ? 'policy' OR profile ? 'rules';

-- A connector's class only chose per-call permissions.
WITH changed AS (
    UPDATE connectors SET document = document - 'class', version = version + 1, updated_at = CURRENT_TIMESTAMP
    WHERE document ? 'class'
    RETURNING name, version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'connectors', name, 'replace', version - 1, version, true FROM changed;

-- The park delay: its override row, the key, and the timer each sandbox was admitted with.
WITH removed AS (
    DELETE FROM settings WHERE key = 'sandbox.park_after_seconds' RETURNING version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'limits', 'sandbox.park_after_seconds', 'delete', version, NULL, true FROM removed;
ALTER TABLE settings DROP CONSTRAINT settings_key_check;
ALTER TABLE settings ADD CONSTRAINT settings_key_check CHECK (key IN (
    'admission.slots', 'admission.per_user', 'sandbox.cpu_millicores', 'sandbox.memory_bytes', 'sandbox.pids',
    'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds', 'mcp.calls_per_minute'));
ALTER TABLE sandbox_instances DROP COLUMN park_after_seconds;

-- The workspace push setting: a run pushes to a writable workspace's source repository without one.
ALTER TABLE workspaces DROP CONSTRAINT workspace_push_source;
ALTER TABLE workspaces DROP COLUMN push_enabled, DROP COLUMN push_protected_branches;

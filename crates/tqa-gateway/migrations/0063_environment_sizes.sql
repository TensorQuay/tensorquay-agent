-- Environment sizes: a sandbox's CPU, memory, processes and a new workspace's disk belong to its environment, within the
-- installation's file-only [sandbox.ceiling]; unset sizes use the file's defaults.

ALTER TABLE templates
    ADD COLUMN pids integer CHECK (pids BETWEEN 64 AND 65536),
    ADD COLUMN workspace_bytes bigint CHECK (workspace_bytes > 0),
    -- What each size the gateway lowered to the ceiling at start had, in column units ({"cpu_millicores": 12000}),
    -- until an administrator saves the environment: the admin list marks each one, and a ceiling raised again gives
    -- it back.
    ADD COLUMN clamped_from jsonb CHECK (jsonb_typeof(clamped_from) = 'object'),
    -- Sizes this upgrade moved in from the Limits page: when the file's entry for a seeded environment next changes,
    -- a size the entry doesn't name keeps the folded value instead of falling back to the defaults. An administrator's
    -- save clears it: the sizes are theirs from then on, under the usual seed-change rule.
    ADD COLUMN sizes_from_limits boolean NOT NULL DEFAULT false;

-- A workspace's own disk, set when it is created and kept (never resized). NULL, for a workspace created before this,
-- is the installation's default ([workspaces] quota_bytes), as it was.
ALTER TABLE workspaces ADD COLUMN quota_bytes bigint CHECK (quota_bytes > 0);

-- What each sandbox was created with, as its timers are recorded.
ALTER TABLE sandbox_instances
    ADD COLUMN cpu_millicores integer CHECK (cpu_millicores > 0),
    ADD COLUMN memory_bytes bigint CHECK (memory_bytes > 0),
    ADD COLUMN pids integer CHECK (pids > 0);

-- The Limits page's CPU, memory and process overrides move into every environment without a size of its own. A CPU
-- override rounds up to a half CPU (environments size CPUs in halves); memory rises to the 256 MiB every environment
-- needs. A size above the installation's ceiling, which only the file knows, is kept here and lowered by the gateway
-- at start, with a note on the environment. Each environment changed, and each override removed, is audited like a
-- seed change (an upgrade's changes show as made by the configuration file).
WITH overrides AS (
    SELECT
        (SELECT greatest(500, ceil(value / 500.0)::bigint * 500) FROM settings
            WHERE key = 'sandbox.cpu_millicores' AND last_set_by = 'admin') AS cpu,
        (SELECT greatest(value, 268435456) FROM settings
            WHERE key = 'sandbox.memory_bytes' AND last_set_by = 'admin') AS memory,
        (SELECT value FROM settings WHERE key = 'sandbox.pids' AND last_set_by = 'admin') AS pids
), changed AS (
    UPDATE templates t
    SET cpu_millicores = COALESCE(t.cpu_millicores, o.cpu::integer),
        memory_bytes = greatest(COALESCE(t.memory_bytes, o.memory), CASE WHEN t.memory_bytes IS NOT NULL THEN 268435456 END),
        pids = COALESCE(t.pids, o.pids::integer),
        sizes_from_limits = true,
        -- A check of the version before still holds: what a task gets doesn't change (the overrides applied before),
        -- apart from a CPU rounded up to a half.
        checked_version = CASE WHEN t.checked_version = t.version THEN t.version + 1 ELSE t.checked_version END,
        version = t.version + 1, updated_at = CURRENT_TIMESTAMP
    FROM overrides o
    WHERE (t.cpu_millicores IS NULL AND o.cpu IS NOT NULL)
       OR (t.memory_bytes IS NULL AND o.memory IS NOT NULL)
       OR (t.pids IS NULL AND o.pids IS NOT NULL)
       OR t.memory_bytes < 268435456
    RETURNING t.name, t.version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'templates', name, 'replace', version - 1, version, true FROM changed;

WITH removed AS (
    DELETE FROM settings WHERE key IN ('sandbox.cpu_millicores', 'sandbox.memory_bytes', 'sandbox.pids')
    RETURNING key, version, last_set_by
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'limits', key, 'delete', version, NULL, true FROM removed WHERE last_set_by = 'admin';

ALTER TABLE settings DROP CONSTRAINT settings_key_check;
ALTER TABLE settings ADD CONSTRAINT settings_key_check CHECK (key IN (
    'admission.slots', 'admission.per_user', 'sandbox.idle_seconds', 'sandbox.max_lifetime_seconds',
    'mcp.calls_per_minute'));

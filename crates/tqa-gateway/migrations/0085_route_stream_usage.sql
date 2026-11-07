-- Next release: a route's `stream_usage` becomes "on unless set false", and false now keeps `stream_options` out of
-- a Chat request (for a server that refuses the field). Before, false was the default and was never applied to runs:
-- the runtime always asked for usage, and the relay never removed it. So every stored false is the old default, not an
-- administrator's choice, and turns into true once; otherwise the new meaning would silently strip usage from those
-- routes' runs, and budgets would stop seeing their tokens. The version moves on, so a form opened before the upgrade
-- can't write the old value back unseen, and the change is in the audit as the upgrade's ("seed", no person).
-- Only routes an administrator added: a route models.toml seeded is left to the seeder's one-time upgrade, which reads
-- the file again, so a file that leaves the field out gets the new default and an explicit false is honoured.
WITH changed AS (
    UPDATE model_routes
    SET document = jsonb_set(document, '{stream_usage}', 'true'::jsonb),
        version = version + 1,
        updated_at = CURRENT_TIMESTAMP
    WHERE document->'stream_usage' = 'false'::jsonb AND seed_digest IS NULL
    RETURNING name, version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'routes', name, 'replace', version - 1, version, true FROM changed;

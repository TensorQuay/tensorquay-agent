-- ADR-0037 §7, the second of two releases: `git_push`, the Git relay and the Git credentials' routes are gone.
-- The Git credentials an earlier start already copied into host keys (moved_at set, migration 0070) are removed with
-- their sealed values. Those not copied yet stay: the gateway copies them at its next start and removes each original
-- once its key is committed, so an install upgrading from before the copy existed loses none. One that can't become a
-- key (its remote isn't a key's address) stays, unused, and the administrator's status page counts it. The tables and
-- the workspace push columns go in the next release.
DELETE FROM secrets
WHERE ref IN (SELECT secret_ref FROM git_credentials WHERE moved_at IS NOT NULL);
DELETE FROM git_credentials WHERE moved_at IS NOT NULL;

-- The relay's one-use push operations: nothing uses them any more, and a task's purge no longer looks for them.
DELETE FROM git_push_operations;

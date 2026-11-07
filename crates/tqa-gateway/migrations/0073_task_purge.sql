-- Delete means gone (PRD §12 item 18): a deleted task or workspace is purged in the background, and only a content-free
-- audit row stays. This migration makes room for that; the purge itself is `purge.rs`.

-- The queue is the database: a deleted row is an outstanding purge, oldest first; the purge deletes the row last.
CREATE INDEX conversations_deleted ON conversations (updated_at, id) WHERE state = 'deleted';
CREATE INDEX workspaces_deleted ON workspaces (updated_at, id) WHERE state = 'deleted';
-- A purged workspace's forks stay (each is its own copy) and stop naming it.
CREATE INDEX workspaces_forked_from ON workspaces (forked_from) WHERE forked_from IS NOT NULL;

-- The replayable answers of a task's create, rename and environment change, and of its own channel posts, hold its
-- title or description. They name the task here, not in `conversation_id`: that column means "an action of this task's
-- run", and the task's latest such row is its failure state (`latest_action_failure`), which an answer must not hide.
-- The purge deletes them in batches by this column; the cascade is the backstop.
ALTER TABLE idempotency ADD COLUMN about_conversation uuid REFERENCES conversations (id) ON DELETE CASCADE;
CREATE INDEX idempotency_about_conversation ON idempotency (about_conversation) WHERE about_conversation IS NOT NULL;

-- Usage is kept, detached from a purged task: token counts per call and no content, counted per person. `user_id` is
-- set on insert from the task's owner, and by the purge on a task's older rows before the task goes; the Usage queries
-- read `coalesce(usage.user_id, conversations.owner_id)`, so no backfill (one long statement holding the table's lock)
-- is needed here.
ALTER TABLE usage ADD COLUMN user_id uuid REFERENCES users (id);
CREATE INDEX usage_user_idx ON usage (user_id);
ALTER TABLE usage ALTER COLUMN conversation_id DROP NOT NULL;
-- NOT VALID: every existing row met the key this replaces, so the table is not read again under the lock.
ALTER TABLE usage DROP CONSTRAINT usage_conversation_id_fkey,
    ADD CONSTRAINT usage_conversation_id_fkey FOREIGN KEY (conversation_id) REFERENCES conversations (id)
        ON DELETE SET NULL NOT VALID;

-- A purged task's channel posts stay (they are the channel's), with no author name, which can carry its title. The
-- purge finds a task's named posts here instead of reading the whole log.
CREATE INDEX channel_messages_task_author ON channel_messages (author_id)
    WHERE author_kind = 'conversation' AND author_name IS NOT NULL;

-- The delete is audited when it happens: who deleted which task or workspace, never a title or a name. Retention's
-- delete is the system's. The area list is 0068's (host keys) plus `tasks` and `workspaces`, so this must apply after
-- it.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_area_check,
    ADD CONSTRAINT admin_audit_area_check
        CHECK (area IN ('secrets', 'routes', 'egress', 'limits', 'connectors', 'templates', 'profiles',
                        'git_credentials', 'connector_credentials', 'channels', 'decision_engines', 'account',
                        'host_keys', 'tasks', 'workspaces'));
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_actor_kind_check,
    ADD CONSTRAINT admin_audit_actor_kind_check CHECK (actor_kind IN ('admin', 'seed', 'user', 'builtin', 'system'));

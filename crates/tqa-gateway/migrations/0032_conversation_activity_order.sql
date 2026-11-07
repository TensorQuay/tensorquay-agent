-- The task list is most recently active first (089): keyset pages for an owner and for an administrator's `all` view.
CREATE INDEX conversations_owner_activity ON conversations (owner_id, last_activity_at DESC, id DESC);
CREATE INDEX conversations_activity ON conversations (last_activity_at DESC, id DESC);

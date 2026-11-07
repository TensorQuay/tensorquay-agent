-- Short task titles: a task's title comes from its first message, the model (after the first completed run), or a
-- person; only a title still from the message is ever replaced, so a person's rename stands.
ALTER TABLE conversations
    ADD COLUMN title_source text NOT NULL DEFAULT 'message' CHECK (title_source IN ('message', 'model', 'person'));

-- What a usage row paid for: a task's own model calls, or the gateway's title call for it.
ALTER TABLE usage ADD COLUMN kind text NOT NULL DEFAULT 'model' CHECK (kind IN ('model', 'title'));

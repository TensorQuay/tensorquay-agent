-- The keep-alive pin (ARCHITECTURE §4.4): a pinned conversation's sandbox is never idle-retired or evicted.
ALTER TABLE conversations ADD COLUMN pinned boolean NOT NULL DEFAULT false;

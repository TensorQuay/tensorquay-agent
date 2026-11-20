-- A member's name in the channel (ADR-0014's amendment, decided on 20 Nov 2026): an agent member's, chosen by its
-- owner when it is added (its task's title by default) and shown to every member; `@` matches it. Unique in the
-- channel without regard to case. A member added before this has none and shows by its profile's name, as before.
ALTER TABLE channel_members
    ADD COLUMN name text CHECK (name IS NULL OR char_length(name) BETWEEN 1 AND 80);

CREATE UNIQUE INDEX channel_members_name ON channel_members (channel_id, lower(name)) WHERE name IS NOT NULL;

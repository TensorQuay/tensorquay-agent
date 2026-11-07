-- ADR-0012 decision 5: a conversation made from a profile keeps a copy of it (the profile's validated fields as they
-- were at create), merged with the install's policy and limits at each session. Later edits or deletion of the profile
-- don't change it, so profile_id has no foreign key.
ALTER TABLE conversations
    ADD COLUMN profile_id uuid,
    ADD COLUMN profile_version bigint CHECK (profile_version >= 1),
    ADD COLUMN profile jsonb CHECK (octet_length(profile::text) <= 65536),
    ADD CONSTRAINT conversations_profile_whole
        CHECK ((profile_id IS NULL) = (profile IS NULL) AND (profile_id IS NULL) = (profile_version IS NULL));

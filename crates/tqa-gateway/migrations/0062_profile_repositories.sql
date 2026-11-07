-- A profile's `repositories` flag is the one switch for working on a project's repository: it grants `git_push`, which
-- leaves the tools a profile lists. Only shared profiles may have it.

-- A shared profile that listed git_push keeps what its administrator chose, as the flag, and is audited like a seed
-- change; a personal one loses it. Every stored profile states the flag, so a read shows it.
WITH changed AS (
    UPDATE agent_profiles
    SET document = jsonb_set(document, '{tools}', (document->'tools') - 'git_push'::text)
            || jsonb_build_object('repositories', owner_id IS NULL),
        version = version + 1, updated_at = CURRENT_TIMESTAMP
    WHERE jsonb_typeof(document->'tools') = 'array' AND document->'tools' ? 'git_push'
    RETURNING owner_id, document->>'name' AS name, version
)
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version, value_hash_changed)
SELECT gen_random_uuid(), 'seed', NULL, 'profiles', name, 'replace', version - 1, version, true
FROM changed WHERE owner_id IS NULL;
UPDATE agent_profiles SET document = document || '{"repositories": false}'::jsonb WHERE NOT document ? 'repositories';

-- A conversation keeps what its copy gave it (ADR-0012): a copy that listed git_push, or listed no tools at all (so it
-- had every tool), keeps git_push through the flag.
UPDATE conversations
SET profile = CASE
        WHEN jsonb_typeof(profile->'tools') = 'array'
            THEN jsonb_set(profile, '{tools}', (profile->'tools') - 'git_push'::text)
                || jsonb_build_object('repositories', profile->'tools' ? 'git_push')
        ELSE profile || '{"repositories": true}'::jsonb
    END
WHERE profile IS NOT NULL AND NOT profile ? 'repositories';

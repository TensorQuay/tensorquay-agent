-- Built-in environments (the built-in environments design): the gateway registers the environments the install built
-- (Minimal, Workstation, later Data) from the images on the engine host, as last_set_by 'builtin'. A built-in has no
-- seed digest: it follows the image its canonical tag names, and a moved tag is an audited update. Placeholder number:
-- set at merge.
ALTER TABLE templates
    DROP CONSTRAINT templates_last_set_by_check,
    ADD CONSTRAINT templates_last_set_by_check CHECK (last_set_by IN ('seed', 'admin', 'builtin')),
    -- When the built-in's image was built, and when a newer build last replaced it (NULL until one has): the
    -- environments list says "Updated to the image built on <date>".
    ADD COLUMN builtin_image_created timestamptz,
    ADD COLUMN builtin_updated_at timestamptz,
    -- A built-in an administrator edited: theirs now, so rebuilds no longer update it, until they reset it to the
    -- built-in. The file's entry of the same name takes it over and clears this.
    ADD COLUMN customised_builtin boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT templates_customised_is_admin CHECK (NOT customised_builtin OR last_set_by = 'admin'),
    -- An administrator's write or the file's entry takes the name over, and clears both.
    ADD CONSTRAINT templates_builtin_dates CHECK (
        (last_set_by = 'builtin') = (builtin_image_created IS NOT NULL)
        AND (builtin_updated_at IS NULL OR last_set_by = 'builtin'));

-- A built-in an administrator deleted stays deleted by name, whatever is built later; unlike a file seed's tombstone,
-- no digest brings it back (every rebuild would). Adding it again is an administrator's own create.
CREATE TABLE builtin_deletions (
    name text PRIMARY KEY CHECK (name IN ('minimal', 'workstation', 'data')),
    deleted_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- A built-in's registration and updates are audited as their own actor (0049's list, extended).
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_actor_kind_check,
    ADD CONSTRAINT admin_audit_actor_kind_check CHECK (actor_kind IN ('admin', 'seed', 'user', 'builtin'));

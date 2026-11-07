-- Desktop can be deleted as the other built-ins can (GW-I-279): 0065's list of built-ins a deletion is recorded for
-- predates Desktop, so deleting it failed on this check and answered 503. A deleted Desktop now stays deleted by name,
-- whatever is built later, until an administrator adds it again.
ALTER TABLE builtin_deletions
    DROP CONSTRAINT builtin_deletions_name_check,
    ADD CONSTRAINT builtin_deletions_name_check CHECK (name IN ('minimal', 'workstation', 'data', 'desktop'));

-- The deleted built-in's build, kept current by the gateway's registration: the template checks keep it as a base, so
-- an administrator can add Desktop again from its image (it starts its own init, which Workstation's check refuses).
-- NULL for a deletion recorded before this until the next registration finds the build.
ALTER TABLE builtin_deletions ADD COLUMN image text;

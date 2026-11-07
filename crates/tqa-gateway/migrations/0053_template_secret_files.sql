-- What a template check found in the template's own layers (ADR-0032; GW-I-100), as the admin API shows it: paths and
-- rules only, never content.
-- check_secret_files: {items: [{path, rule, acknowledged}], more} for a refused template (image_secret_file: every hit,
-- the acknowledged ones marked) or a verified one (its acknowledged warnings); at most 16 items, `more` counts the rest.
-- check_scan_limit: {bound, limit} for a template refused with image_scan_limit.

ALTER TABLE templates
    ADD COLUMN check_secret_files jsonb
        CHECK (check_secret_files IS NULL OR (jsonb_typeof(check_secret_files) = 'object'
            AND jsonb_array_length(check_secret_files -> 'items') <= 16)),
    ADD COLUMN check_scan_limit jsonb
        CHECK (check_scan_limit IS NULL OR jsonb_typeof(check_scan_limit) = 'object');

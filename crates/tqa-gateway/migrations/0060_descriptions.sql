-- An administrator's one-line description of a model route and of an environment, shown where people pick one
-- (GW-I-132): display only. The gateway checks it (1 through 160 characters, one line, no surrounding spaces); the
-- columns bound its length.
ALTER TABLE model_routes ADD COLUMN description text
    CHECK (description IS NULL OR char_length(description) BETWEEN 1 AND 160);
ALTER TABLE templates ADD COLUMN description text
    CHECK (description IS NULL OR char_length(description) BETWEEN 1 AND 160);

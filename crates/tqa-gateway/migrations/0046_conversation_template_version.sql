-- ADR-0032 decision 7: the template version whose image a conversation was last moved to (NULL until it moves).
ALTER TABLE conversations ADD COLUMN template_version bigint;

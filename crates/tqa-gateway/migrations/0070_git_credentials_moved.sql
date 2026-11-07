-- ADR-0037 §7, the first of two releases (guardrails §6): the gateway copies each Git credential into a host key at
-- its start and marks it here, so it is copied once. The credentials stay, readable, for one release; the next removes
-- them.
ALTER TABLE git_credentials ADD COLUMN moved_at timestamptz;

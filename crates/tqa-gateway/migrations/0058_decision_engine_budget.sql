-- An engine's budget (ADR-0013, the adapter amendment; the lead's rules of 29 Sep). `total` counts the engine's usage
-- from budget_since, when the current budget was set or last changed, so saving a new budget starts a new count; a
-- calendar month counts the whole month whatever changed in it. NULL when the engine has no budget. An engine that
-- already has one counts from its last change (updated_at), the best time known and never before its real start.
ALTER TABLE decision_engines ADD COLUMN budget_since timestamptz;
UPDATE decision_engines SET budget_since = updated_at WHERE document ? 'budget';
-- The relay counts an engine's use on every budgeted consultation (and the administrators' list shows it): by route
-- over a time range, which this index answers without reading every model call's row.
CREATE INDEX usage_route_created_at ON usage (route, created_at);

-- Every change of an engine's budget is its own action, like its exposure (0056's list, extended).
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_action_check,
    ADD CONSTRAINT admin_audit_action_check
        CHECK (action IN ('create', 'replace', 'delete', 'probe', 'list_models', 'exposure', 'budget'));
-- A budget's detail is `none` or `calls<N>_tokens<N>_<period>` (absent limits left out) on each side, such as
-- `none->calls3000_month`: numbers and names only, never text a person typed. The exposure's details still match.
ALTER TABLE admin_audit
    DROP CONSTRAINT admin_audit_detail_check,
    ADD CONSTRAINT admin_audit_detail_check CHECK (detail IS NULL OR detail ~ '^[a-z0-9_]{1,64}->[a-z0-9_]{1,64}$');

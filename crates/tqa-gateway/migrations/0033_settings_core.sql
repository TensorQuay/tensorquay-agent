-- ADR-0028: the settings generation stamps each snapshot; every settings write bumps it in its own transaction.
CREATE TABLE settings_generation (
    id smallint PRIMARY KEY CHECK (id = 1),
    generation bigint NOT NULL CHECK (generation >= 0)
);
INSERT INTO settings_generation (id, generation) VALUES (1, 0);

-- The audit covers every settings area.
ALTER TABLE admin_audit DROP CONSTRAINT admin_audit_area_check;
ALTER TABLE admin_audit ADD CONSTRAINT admin_audit_area_check CHECK (area IN ('secrets', 'routes', 'egress', 'limits'));

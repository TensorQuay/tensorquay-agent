-- An environment declares up to six capabilities, the protocol's bound since the 'data' capability (revision 10):
-- Workstation declares all six, and the five this column allowed refused it at start-up (candidate 13, GW-I-159).
ALTER TABLE templates
    DROP CONSTRAINT templates_capabilities_check,
    ADD CONSTRAINT templates_capabilities_check CHECK (cardinality(capabilities) <= 6);

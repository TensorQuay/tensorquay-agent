-- An environment declares up to seven capabilities, the protocol's bound since the 'desktop' capability (revision 14,
-- ADR-0039): the Desktop built-in declares all seven, which the six this column allowed would refuse at start-up.
ALTER TABLE templates
    DROP CONSTRAINT templates_capabilities_check,
    ADD CONSTRAINT templates_capabilities_check CHECK (cardinality(capabilities) <= 7);

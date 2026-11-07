-- An administrator may let a rule for one exact host NAME reach a private address (ADR-0035 amendment, the owner's
-- decision of 1 Oct 2026). Off unless set; never on a wildcard, the any-host rule or an address.
ALTER TABLE egress_rules ADD COLUMN internal boolean NOT NULL DEFAULT false;
ALTER TABLE egress_rules ADD CONSTRAINT egress_rules_internal_names_only CHECK (
    NOT internal OR (host NOT LIKE '*%' AND host !~ '^[0-9.]+$' AND position(':' IN host) = 0)
);

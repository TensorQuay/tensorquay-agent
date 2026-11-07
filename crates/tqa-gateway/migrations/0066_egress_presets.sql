-- F20: internet-access presets, built-in named rule sets. A rule remembers the preset that added it, and whether an
-- administrator has edited it since (it is then theirs: removing the preset keeps it). Only the open_web preset stores
-- the any-host rule '*', and nothing else can.
ALTER TABLE egress_rules
    ADD COLUMN preset text CHECK (preset IN ('package_registries', 'open_web', 'code_hosting')),
    ADD COLUMN preset_edited boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT egress_rules_any_host_by_preset CHECK (host <> '*' OR preset = 'open_web');

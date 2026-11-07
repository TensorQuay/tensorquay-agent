-- A customised built-in keeps the build it was customised from as its base (the c16i Desktop resize, 14 Oct 2026). An
-- administrator's edit took a built-in out of the template checks' bases, so Desktop, built FROM Workstation with its
-- own entrypoint, was checked against Workstation, refused (`image_entrypoint`) and gone from the picker. NULL on every
-- other template; a built-in customised before this has none until the gateway's next registration finds it still on
-- the image its tag names.
ALTER TABLE templates
    ADD COLUMN builtin_base_image text,
    ADD CONSTRAINT templates_builtin_base_customised CHECK (builtin_base_image IS NULL OR customised_builtin);

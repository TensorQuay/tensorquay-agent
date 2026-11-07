`lib/old_api` is deprecated. Migrate every caller under `app/` to `lib/new_api`, following `docs/MIGRATION.md`.
Every endpoint must return exactly the same line as before, and nothing under `app/` may use `old_api` any more.
Do not change `lib/` or the documentation. Run the tests with `python3 -m unittest discover -s tests -t .`.

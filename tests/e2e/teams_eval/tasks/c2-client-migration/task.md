`client/v1.py` is deprecated. Migrate its four callers under `app/` to `client/v2.py`, following
`docs/MIGRATION.md`. Every function must behave exactly as before, including when the store has nothing at the path,
and nothing under `app/` may use `v1` any more. Do not change `client/` or the documentation. Run the tests with
`python3 -m unittest discover -s tests -t .`.

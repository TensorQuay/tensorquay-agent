Add a `--format` option to `report.py` with two values: `text`, the default, which keeps today's output unchanged, and
`csv`. CSV output is the header line `region,total` followed by one `region,total` line per region, in the same order
as the text output. Any other value must be rejected with an error.

Add a test for the CSV output under `tests/`. Run the tests with `python3 -m unittest discover -s tests -t .`.

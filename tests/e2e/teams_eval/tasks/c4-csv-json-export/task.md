Add a `--format` option to `report.py` with three values: `text`, the default, which keeps today's output
unchanged; `csv`, the header line `region,total` followed by one `region,total` line per region; and `json`, a list
of `{"region": ..., "total": ...}` objects. Both keep the text output's order. Any other value must be rejected
with an error. Add tests for the CSV and JSON output under `tests/`, and document the option in `README.md`'s Usage
section. Run the tests with `python3 -m unittest discover -s tests -t .`.

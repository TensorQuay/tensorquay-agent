# report

Sales totals by region.

## Usage

```sh
python3 report.py sales.json
```

The output is one `region: total` line per region, sorted by region.

`--format csv` prints `region,total` lines after a header, and `--format json` a list of `{"region", "total"}` objects; `--format text` is the default.

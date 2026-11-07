# The weekly service report

`pipeline/` turns the logs in `logs/` into the week's report, in four stages, each in its own module:

1. **parse** (`pipeline/parse.py`): each log format's lines into raw records:
   - `logs/access.log`: the web tier (services `api` and `search`), one request a line;
   - `logs/app.jsonl`: the application services (`billing`, `auth`), one JSON event a line; only `"event": "request"`
     lines are requests;
   - `logs/worker.syslog`: the background workers (`worker`, `mailer`), syslog lines without a year: they're all from
     2026, in UTC.
2. **normalise** (`pipeline/normalize.py`): every raw record into one record shape, the same for all three sources.
3. **aggregate** (`pipeline/aggregate.py`): per service, for the week starting Monday `--week` (UTC, 7 days):
   - `requests`: how many;
   - `errors`: those with an HTTP status of 500 or more, or a job that failed;
   - `p95_ms`: the 95th percentile of the duration in milliseconds, nearest rank (the value at position ⌈0.95 × n⌉ of
     the sorted durations), as an integer;
   - `users`: how many distinct users.
4. **report** (`pipeline/report.py`): `weekly.json` (service → the four figures) and `weekly.md` (one table) in the
   output folder.

`python3 -m pipeline.run logs out --week 2026-09-28` runs all four. The normalised record's fields are the builders'
choice, but every stage must agree on them.

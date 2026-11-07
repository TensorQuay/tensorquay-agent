Build the weekly service report in `pipeline/`: the four stages described in `docs/PIPELINE.md` (parse, normalise,
aggregate, report), each in its own module, agreeing on the records they hand each other. Then run it on `logs/` for
the week starting Monday 2026-09-28 (`python3 -m pipeline.run logs out --week 2026-09-28`), so `out/weekly.json` and
`out/weekly.md` hold that week's figures.

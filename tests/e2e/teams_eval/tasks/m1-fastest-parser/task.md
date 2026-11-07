The folder `parsers/` holds three CSV parsers that give the same rows on `data/sales.csv`. Find which is fastest on
that file and switch `tool/report.py` to it, keeping its output identical. Add `bench.py` at the top of the
workspace: it times all three parsers on `data/sales.csv` and prints each one's time; it must run with
`python3 bench.py`.

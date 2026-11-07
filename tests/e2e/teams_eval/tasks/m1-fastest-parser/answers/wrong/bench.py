"""Times the three parsers on data/sales.csv: python3 bench.py."""
import time
from pathlib import Path

from parsers import char_parser, csv_parser, split_parser

TEXT = (Path(__file__).resolve().parent / "data" / "sales.csv").read_text()
results = {}
for name, module in (("csv_parser", csv_parser), ("split_parser", split_parser), ("char_parser", char_parser)):
    started = time.perf_counter()
    module.parse(TEXT)
    results[name] = time.perf_counter() - started
for name, seconds in sorted(results.items(), key=lambda item: item[1]):
    print(f"{name}: {seconds * 1000:.1f} ms")

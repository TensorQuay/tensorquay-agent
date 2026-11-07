"""Writes data/events.jsonl's events with the writer, and out/summary.json as the reader reads them."""
import json
from pathlib import Path

from report.evlog_report import summarize
from writer.evlog_writer import write_events

events = [json.loads(line) for line in Path("data/events.jsonl").read_text(encoding="utf-8").splitlines() if line]
Path("out").mkdir(exist_ok=True)
write_events("out/events.evl", events)
Path("out/summary.json").write_text(json.dumps(summarize("out/events.evl"), indent=1, ensure_ascii=False,
                                               sort_keys=True) + "\n", encoding="utf-8")

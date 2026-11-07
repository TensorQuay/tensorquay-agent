Build the event log's writer (`writer/evlog_writer.py`) and reader (`report/evlog_report.py`) to the format in
`spec/FORMAT.md`. The spec leaves some points open: settle them so that every file the writer writes is read back
exactly by the reader, including non-ASCII text, long messages and an empty log, and record your decisions in the
spec's Decisions section. Then write the events of `data/events.jsonl` with the writer, read them back with the
reader, and save the reader's summary to `out/summary.json`. Add tests to `tests/`; run them with `python3 -m unittest discover -s tests -t .`.

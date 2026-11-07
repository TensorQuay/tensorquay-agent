"""python3 -m pipeline.run LOGS OUT --week YYYY-MM-DD: the four stages in order."""
import argparse
from pathlib import Path

from pipeline import aggregate, normalize, parse, report


def main(logs, out, week):
    logs = Path(logs)
    records = []
    for name, source, reader in (("access.log", "access", parse.parse_access), ("app.jsonl", "app", parse.parse_app),
                                 ("worker.syslog", "worker", parse.parse_worker)):
        lines = (logs / name).read_text(encoding="utf-8").splitlines()
        records += normalize.normalize(reader(lines), source)
    figures = aggregate.aggregate(records, week)
    report.write(figures, Path(out))
    return figures


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("logs")
    parser.add_argument("out")
    parser.add_argument("--week", required=True)
    arguments = parser.parse_args()
    main(arguments.logs, arguments.out, arguments.week)

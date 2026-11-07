"""Print sales totals by region from a JSON file of {region, amount} rows."""
import argparse
import csv
import io
import json


def load(path):
    with open(path) as file:
        return json.load(file)


def totals(rows):
    """Return (region, total) pairs sorted by region."""
    sums = {}
    for row in rows:
        sums[row["region"]] = sums.get(row["region"], 0) + row["amount"]
    return sorted(sums.items())


def render_text(pairs):
    return "\n".join(f"{region}: {total}" for region, total in pairs)


def render_csv(pairs):
    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\n")
    writer.writerow(("region", "total"))
    writer.writerows(pairs)
    return out.getvalue().rstrip("\n")


def render_json(pairs):
    return json.dumps([{"region": region, "total": total} for region, total in pairs], indent=2)


RENDER = {"text": render_text, "csv": render_csv, "json": render_json}


def main(argv=None):
    parser = argparse.ArgumentParser(prog="report", description="Sales totals by region.")
    parser.add_argument("path", help="a JSON file of {region, amount} rows")
    parser.add_argument("--format", choices=sorted(RENDER), default="text")
    args = parser.parse_args(argv)
    print(RENDER[args.format](totals(load(args.path))))


if __name__ == "__main__":
    main()

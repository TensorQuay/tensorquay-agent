"""Print sales totals by region from a JSON file of {region, amount} rows."""
import argparse
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


def main(argv=None):
    parser = argparse.ArgumentParser(prog="report", description="Sales totals by region.")
    parser.add_argument("path", help="a JSON file of {region, amount} rows")
    args = parser.parse_args(argv)
    print(render_text(totals(load(args.path))))


if __name__ == "__main__":
    main()

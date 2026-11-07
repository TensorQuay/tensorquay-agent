"""Units sold per category, from data/sales.csv."""
from pathlib import Path

from parsers.char_parser import parse


def units_by_category(text):
    totals = {}
    for category, _, units in parse(text)[1:]:
        totals[category] = totals.get(category, 0) + int(units)
    return dict(sorted(totals.items()))


def main():
    text = (Path(__file__).resolve().parent.parent / "data" / "sales.csv").read_text()
    for category, units in units_by_category(text).items():
        print(f"{category}: {units}")


if __name__ == "__main__":
    main()

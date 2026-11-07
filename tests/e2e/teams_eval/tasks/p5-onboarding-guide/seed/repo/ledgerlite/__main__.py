import argparse
import sys

from ledgerlite.book import Book


def main(argv=None):
    parser = argparse.ArgumentParser(prog="ledgerlite", description="A tiny ledger.")
    parser.add_argument("--file", default="ledger.json")
    sub = parser.add_subparsers(dest="command", required=True)
    add = sub.add_parser("add")
    add.add_argument("amount", type=float)
    add.add_argument("note")
    sub.add_parser("balance")
    args = parser.parse_args(argv)
    book = Book.load(args.file)
    if args.command == "add":
        book.add(args.amount, args.note)
        book.save(args.file)
    print(f"{book.balance():.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

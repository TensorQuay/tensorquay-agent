"""Print the lines of a file that contain a pattern."""
import argparse


def main(argv=None):
    parser = argparse.ArgumentParser(prog="find_lines", description="Print the lines of FILE that contain PATTERN.")
    parser.add_argument("pattern", help="the text to look for")
    parser.add_argument("file", help="the file to search")
    parser.add_argument("--ignore-case", action="store_true", help="match without regard to case")
    parser.add_argument("--limit", type=int, metavar="N", help="stop after N matching lines")
    parser.add_argument("--numbers", action="store_true", help="prefix each line with its line number")
    args = parser.parse_args(argv)
    needle = args.pattern.lower() if args.ignore_case else args.pattern
    shown = 0
    with open(args.file) as lines:
        for number, line in enumerate(lines, start=1):
            haystack = line.lower() if args.ignore_case else line
            if needle in haystack:
                print(f"{number}: {line.rstrip()}" if args.numbers else line.rstrip())
                shown += 1
                if args.limit is not None and shown >= args.limit:
                    break


if __name__ == "__main__":
    main()

# Onboarding

## Setup

You need Python 3.10 or later; there are no dependencies.

## Project layout

- `ledgerlite/book.py`: the ledger
- `ledgerlite/__main__.py`: the command line
- `tests/`: unit tests

## Running the tests

```sh
python3 -m unittest discover -s test -t .
```

## Using the tool

```sh
python3 -m ledgerlite --help
python3 -m ledgerlite --file /tmp/l.json add 5 coffee
```

## Making a change

Branch from main and add a test with every change (`CONTRIBUTING.md`).

"""CSV rows with the standard library's csv module."""
import csv
import io


def parse(text):
    return list(csv.reader(io.StringIO(text)))

"""CSV rows one character at a time."""


def parse(text):
    rows, row, field = [], [], ""
    for char in text:
        if char == ",":
            row = row + [field]
            field = ""
        elif char == "\n":
            rows = rows + [row + [field]]
            row, field = [], ""
        else:
            field = field + char
    if field or row:
        rows = rows + [row + [field]]
    return rows

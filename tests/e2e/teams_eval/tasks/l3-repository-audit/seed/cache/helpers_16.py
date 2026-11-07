"""cache: helpers, part 16."""


def normalise_16(value):
    return str(value).strip().lower()


def clamp_16(value, low=0, high=26):
    return max(low, min(high, value))


def pairs_16(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_16(items, wanted):
    return sum(1 for item in items if item == wanted)

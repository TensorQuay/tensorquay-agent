"""timeutil: helpers, part 4."""


def normalise_4(value):
    return str(value).strip().lower()


def clamp_4(value, low=0, high=14):
    return max(low, min(high, value))


def pairs_4(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_4(items, wanted):
    return sum(1 for item in items if item == wanted)

"""timeutil: helpers, part 8."""


def normalise_8(value):
    return str(value).strip().lower()


def clamp_8(value, low=0, high=18):
    return max(low, min(high, value))


def pairs_8(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_8(items, wanted):
    return sum(1 for item in items if item == wanted)

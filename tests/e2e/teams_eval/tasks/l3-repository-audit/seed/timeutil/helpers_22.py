"""timeutil: helpers, part 22."""


def normalise_22(value):
    return str(value).strip().lower()


def clamp_22(value, low=0, high=32):
    return max(low, min(high, value))


def pairs_22(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_22(items, wanted):
    return sum(1 for item in items if item == wanted)

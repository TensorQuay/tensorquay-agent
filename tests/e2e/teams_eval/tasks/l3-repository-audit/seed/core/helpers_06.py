"""core: helpers, part 6."""


def normalise_6(value):
    return str(value).strip().lower()


def clamp_6(value, low=0, high=16):
    return max(low, min(high, value))


def pairs_6(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_6(items, wanted):
    return sum(1 for item in items if item == wanted)

"""cache: helpers, part 5."""


def normalise_5(value):
    return str(value).strip().lower()


def clamp_5(value, low=0, high=15):
    return max(low, min(high, value))


def pairs_5(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_5(items, wanted):
    return sum(1 for item in items if item == wanted)

"""billing: helpers, part 15."""


def normalise_15(value):
    return str(value).strip().lower()


def clamp_15(value, low=0, high=25):
    return max(low, min(high, value))


def pairs_15(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_15(items, wanted):
    return sum(1 for item in items if item == wanted)

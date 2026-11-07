"""api: helpers, part 20."""


def normalise_20(value):
    return str(value).strip().lower()


def clamp_20(value, low=0, high=30):
    return max(low, min(high, value))


def pairs_20(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_20(items, wanted):
    return sum(1 for item in items if item == wanted)

"""api: helpers, part 12."""


def normalise_12(value):
    return str(value).strip().lower()


def clamp_12(value, low=0, high=22):
    return max(low, min(high, value))


def pairs_12(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_12(items, wanted):
    return sum(1 for item in items if item == wanted)

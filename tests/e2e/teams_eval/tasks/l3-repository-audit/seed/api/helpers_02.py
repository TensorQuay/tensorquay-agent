"""api: helpers, part 2."""


def normalise_2(value):
    return str(value).strip().lower()


def clamp_2(value, low=0, high=12):
    return max(low, min(high, value))


def pairs_2(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_2(items, wanted):
    return sum(1 for item in items if item == wanted)

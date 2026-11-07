"""util: helpers, part 1."""


def normalise_1(value):
    return str(value).strip().lower()


def clamp_1(value, low=0, high=11):
    return max(low, min(high, value))


def pairs_1(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_1(items, wanted):
    return sum(1 for item in items if item == wanted)

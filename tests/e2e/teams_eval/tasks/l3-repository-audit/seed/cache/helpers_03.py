"""cache: helpers, part 3."""


def normalise_3(value):
    return str(value).strip().lower()


def clamp_3(value, low=0, high=13):
    return max(low, min(high, value))


def pairs_3(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_3(items, wanted):
    return sum(1 for item in items if item == wanted)

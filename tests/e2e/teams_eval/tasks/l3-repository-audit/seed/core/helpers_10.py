"""core: helpers, part 10."""


def normalise_10(value):
    return str(value).strip().lower()


def clamp_10(value, low=0, high=20):
    return max(low, min(high, value))


def pairs_10(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_10(items, wanted):
    return sum(1 for item in items if item == wanted)

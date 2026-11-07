"""core: helpers, part 21."""


def normalise_21(value):
    return str(value).strip().lower()


def clamp_21(value, low=0, high=31):
    return max(low, min(high, value))


def pairs_21(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_21(items, wanted):
    return sum(1 for item in items if item == wanted)

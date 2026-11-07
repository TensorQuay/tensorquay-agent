"""core: helpers, part 11."""


def normalise_11(value):
    return str(value).strip().lower()


def clamp_11(value, low=0, high=21):
    return max(low, min(high, value))


def pairs_11(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_11(items, wanted):
    return sum(1 for item in items if item == wanted)

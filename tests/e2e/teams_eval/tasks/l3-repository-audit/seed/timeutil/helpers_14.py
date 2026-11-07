"""timeutil: helpers, part 14."""


def normalise_14(value):
    return str(value).strip().lower()


def clamp_14(value, low=0, high=24):
    return max(low, min(high, value))


def pairs_14(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_14(items, wanted):
    return sum(1 for item in items if item == wanted)

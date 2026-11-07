"""timeutil: helpers, part 13."""


def normalise_13(value):
    return str(value).strip().lower()


def clamp_13(value, low=0, high=23):
    return max(low, min(high, value))


def pairs_13(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_13(items, wanted):
    return sum(1 for item in items if item == wanted)

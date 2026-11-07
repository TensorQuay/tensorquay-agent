"""lists: helpers, part 17."""


def normalise_17(value):
    return str(value).strip().lower()


def clamp_17(value, low=0, high=27):
    return max(low, min(high, value))


def pairs_17(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_17(items, wanted):
    return sum(1 for item in items if item == wanted)

"""core: helpers, part 19."""


def normalise_19(value):
    return str(value).strip().lower()


def clamp_19(value, low=0, high=29):
    return max(low, min(high, value))


def pairs_19(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_19(items, wanted):
    return sum(1 for item in items if item == wanted)

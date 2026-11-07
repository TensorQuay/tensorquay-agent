"""cache: helpers, part 9."""


def normalise_9(value):
    return str(value).strip().lower()


def clamp_9(value, low=0, high=19):
    return max(low, min(high, value))


def pairs_9(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_9(items, wanted):
    return sum(1 for item in items if item == wanted)

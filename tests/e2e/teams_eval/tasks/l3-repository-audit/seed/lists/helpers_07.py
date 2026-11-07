"""lists: helpers, part 7."""


def normalise_7(value):
    return str(value).strip().lower()


def clamp_7(value, low=0, high=17):
    return max(low, min(high, value))


def pairs_7(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_7(items, wanted):
    return sum(1 for item in items if item == wanted)

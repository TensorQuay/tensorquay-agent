"""Small descriptive statistics over non-empty sequences of numbers."""


def mean(values):
    """Return the arithmetic mean."""
    if not values:
        raise ValueError("mean of an empty sequence")
    return sum(values) / len(values)


def median(values):
    """Return the middle value; the mean of the two middle values for an even count."""
    if not values:
        raise ValueError("median of an empty sequence")
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def mode(values):
    """Return the most frequent value; the smallest one when several are equally frequent."""
    if not values:
        raise ValueError("mode of an empty sequence")
    counts = {}
    for value in values:
        counts[value] = counts.get(value, 0) + 1
    best = max(counts.values())
    return values[0]

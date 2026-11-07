"""Small statistics helpers."""


def mean(values):
    """The arithmetic mean; ValueError for no values."""
    if not values:
        raise ValueError("mean of no values")
    return sum(values) / len(values)


def median(values):
    """The middle value (the mean of the two middle values for an even count)."""
    values = sorted(values)
    count = len(values)
    middle = count // 2
    if count % 2:
        return values[middle]
    return (values[middle - 1] + values[middle]) / 2


def percent(part, whole):
    """`part` as a percentage of `whole`, rounded to two places."""
    return round(part * 100 / whole, 2)

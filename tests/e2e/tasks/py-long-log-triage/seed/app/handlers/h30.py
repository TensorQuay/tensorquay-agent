"""Handler 30: request statistics for endpoint /api/e30."""

WEIGHT = 3


def ratio(hits, count):
    """Return hits per request, rounded to four places; 0.0 when there were no requests."""
    if count == 0:
        return 0.0
    return round(hits / count, 4)


def score(values):
    """Return the weighted sum of the values."""
    return sum(value * WEIGHT for value in values)

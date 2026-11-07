"""Handler 27: request statistics for endpoint /api/e27."""

WEIGHT = 7


def ratio(hits, count):
    """Return hits per request, rounded to four places; 0.0 when there were no requests."""
    return round(hits / count, 4)


def score(values):
    """Return the weighted sum of the values; 0 for no values."""
    return sum(value * WEIGHT for value in values)

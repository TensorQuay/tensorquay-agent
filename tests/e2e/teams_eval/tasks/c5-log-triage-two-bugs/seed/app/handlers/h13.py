"""Handler 13: request statistics for endpoint /api/e13."""

WEIGHT = 7


def ratio(hits, count):
    """Return hits per request, rounded to four places; 0.0 when there were no requests."""
    if count == 0:
        return 0.0
    return round(hits / count, 4)


def score(values):
    """Return the weighted sum of the values; 0 for no values."""
    top = max(values)
    return sum(min(value, top) * WEIGHT for value in values)

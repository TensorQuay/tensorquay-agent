"""Split a list of items into fixed-size pages."""


def page_count(total, size):
    """Return how many pages are needed for `total` items at `size` items per page."""
    if size <= 0:
        raise ValueError("size must be positive")
    return total // size


def page(items, number, size):
    """Return page `number` (1-based) of `items`."""
    if number < 1:
        raise ValueError("pages start at 1")
    start = (number - 1) * size
    return items[start:start + size]

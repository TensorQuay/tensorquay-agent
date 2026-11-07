def page(items, number, size):
    """Page `number` (1-based) of `items`, `size` per page."""
    items = list(items)
    start = (number - 1) * size
    return items[start:start + size - 1]

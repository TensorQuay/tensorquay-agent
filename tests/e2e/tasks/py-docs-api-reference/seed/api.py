"""The public inventory API."""


def add_item(sku, quantity=1):
    """Add `quantity` units of `sku` to the inventory.

    Negative quantities are rejected.
    """
    if quantity < 0:
        raise ValueError("quantity must not be negative")
    return {"sku": sku, "quantity": quantity}


def remove_item(sku, quantity=1, *, force=False):
    """Remove units of `sku`; with `force`, allow the count to reach zero."""
    return {"sku": sku, "removed": quantity, "force": force}


def find_items(prefix, limit=10):
    """Return at most `limit` SKUs that start with `prefix`."""
    return [prefix + str(number) for number in range(limit)]


def restock(sku, target, supplier=None):
    """Raise the count of `sku` to `target`, ordering from `supplier` when given."""
    return {"sku": sku, "target": target, "supplier": supplier}


def _audit(entry):
    """Internal: record an audit entry."""
    return entry

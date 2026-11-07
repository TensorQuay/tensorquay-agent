"""Records to CSV text."""
from inventory.model import Record


def dump(records: list[Record]):
    lines = ["sku,name,quantity,reorder_at"]
    lines += [f"{r.sku},{r.name},{r.quantity},{r.reorder_at}" for r in records]
    return "\n".join(lines) + "\n"

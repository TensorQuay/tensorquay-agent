"""Stock reports."""
from inventory.model import Record


def low_stock(records: list[Record]) -> list[Record]:
    """The Records at or under their reorder level, by SKU."""
    return sorted((r for r in records if r.needs_reorder()), key=lambda r: r.sku)


def summary(records: list[Record]):
    return f"{len(records)} records, {len(low_stock(records))} to reorder"

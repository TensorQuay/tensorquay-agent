"""Stock reports."""
from inventory.model import StockItem


def low_stock(records: list[StockItem]) -> list[StockItem]:
    """The StockItems at or under their reorder level, by SKU."""
    return sorted((r for r in records if r.needs_reorder()), key=lambda r: r.sku)


def summary(records: list[StockItem]):
    return f"{len(records)} records, {len(low_stock(records))} to reorder"

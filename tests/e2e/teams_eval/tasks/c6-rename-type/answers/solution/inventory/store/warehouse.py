"""StockItems held in a warehouse."""
from inventory.model import StockItem


class Warehouse:
    def __init__(self):
        self._records = {}

    def add(self, record: StockItem):
        self._records[record.sku] = record

    def find(self, sku) -> StockItem | None:
        return self._records.get(sku)

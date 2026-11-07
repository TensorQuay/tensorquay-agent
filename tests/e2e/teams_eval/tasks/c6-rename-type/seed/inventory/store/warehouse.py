"""Records held in a warehouse."""
from inventory.model import Record


class Warehouse:
    def __init__(self):
        self._records = {}

    def add(self, record: Record):
        self._records[record.sku] = record

    def find(self, sku) -> Record | None:
        return self._records.get(sku)

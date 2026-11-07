"""The stock record."""
from dataclasses import dataclass


@dataclass
class StockItem:
    sku: str
    name: str
    quantity: int
    reorder_at: int = 5

    def needs_reorder(self):
        return self.quantity <= self.reorder_at

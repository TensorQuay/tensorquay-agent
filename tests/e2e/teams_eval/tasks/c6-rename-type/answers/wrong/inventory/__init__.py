"""Stock records for a small shop."""
from inventory.io.loader import load
from inventory.model import StockItem
from inventory.reports.stock import low_stock

# Record stays as an alias of StockItem for existing users.
Record = StockItem

__all__ = ["Record", "StockItem", "load", "low_stock"]

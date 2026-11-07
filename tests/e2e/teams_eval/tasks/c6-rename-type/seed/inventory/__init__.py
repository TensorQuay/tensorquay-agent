"""Stock records for a small shop."""
from inventory.io.loader import load
from inventory.model import Record
from inventory.reports.stock import low_stock

__all__ = ["Record", "load", "low_stock"]

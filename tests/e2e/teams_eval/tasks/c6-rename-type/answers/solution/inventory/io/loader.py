"""StockItems from CSV text."""
import csv
import io

from inventory.model import StockItem


def load(text):
    """Each CSV row (sku,name,quantity,reorder_at) as a StockItem."""
    rows = csv.DictReader(io.StringIO(text))
    return [StockItem(row["sku"], row["name"], int(row["quantity"]), int(row["reorder_at"])) for row in rows]

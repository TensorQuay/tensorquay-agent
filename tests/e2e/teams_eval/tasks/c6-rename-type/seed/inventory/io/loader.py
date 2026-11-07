"""Records from CSV text."""
import csv
import io

from inventory.model import Record


def load(text):
    """Each CSV row (sku,name,quantity,reorder_at) as a Record."""
    rows = csv.DictReader(io.StringIO(text))
    return [Record(row["sku"], row["name"], int(row["quantity"]), int(row["reorder_at"])) for row in rows]

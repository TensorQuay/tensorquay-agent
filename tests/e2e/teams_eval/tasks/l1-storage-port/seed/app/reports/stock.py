"""Stock: units per warehouse city, and the products below a level."""
from app.db import connect


def units_by_city(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT w.city AS city, SUM(s.quantity) AS units FROM stocks s "
                                  "JOIN warehouses w ON w.id = s.warehouse_id GROUP BY w.city ORDER BY w.city")
        return {row["city"]: row["units"] for row in rows}


def low_stock(path, level):
    with connect(path) as connection:
        rows = connection.execute("SELECT p.sku AS sku, SUM(s.quantity) AS units FROM stocks s "
                                  "JOIN products p ON p.id = s.product_id GROUP BY p.id HAVING units < ? "
                                  "ORDER BY p.sku", (level,))
        return [row["sku"] for row in rows]

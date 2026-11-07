"""Sales: revenue per customer city, and the best customers."""
from app.db import connect


def revenue_by_city(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.city AS city, SUM(o.total_cents) AS cents FROM orders o "
                                  "JOIN customers c ON c.id = o.customer_id WHERE o.status = 'paid' "
                                  "GROUP BY c.city ORDER BY c.city")
        return {row["city"]: row["cents"] for row in rows}


def best_customers(path, limit=3):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.name AS name, SUM(o.total_cents) AS cents FROM orders o "
                                  "JOIN customers c ON c.id = o.customer_id WHERE o.status = 'paid' "
                                  "GROUP BY c.id ORDER BY cents DESC, c.name LIMIT ?", (limit,))
        return [(row["name"], row["cents"]) for row in rows]

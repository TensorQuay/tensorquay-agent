"""Support: open tickets per customer, and products' average stars."""
from app.db import connect


def open_tickets(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.email AS email, COUNT(*) AS n FROM tickets t "
                                  "JOIN customers c ON c.id = t.customer_id WHERE t.state = 'open' "
                                  "GROUP BY c.id ORDER BY c.email")
        return {row["email"]: row["n"] for row in rows}


def average_stars(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT p.sku AS sku, AVG(r.stars) AS stars FROM reviews r "
                                  "JOIN products p ON p.id = r.product_id GROUP BY p.id ORDER BY p.sku")
        return {row["sku"]: round(row["stars"], 2) for row in rows}

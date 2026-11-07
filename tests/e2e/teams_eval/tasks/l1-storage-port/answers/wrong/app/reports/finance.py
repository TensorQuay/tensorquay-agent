"""Finance: refunds against payments per order, and unpaid invoices."""
from app.db import connect


def net_by_order(path):
    with connect(path) as connection:
        paid = {row["order_id"]: row["cents"] for row in connection.execute(
            "SELECT order_id, SUM(amount_cents) AS cents FROM payments GROUP BY order_id")}
        refunded = {row["order_id"]: row["cents"] for row in connection.execute(
            "SELECT order_id, SUM(amount_cents) AS cents FROM refunds GROUP BY order_id")}
        return {order: paid[order] - refunded.get(order, 0) for order in sorted(paid)}


def unpaid_invoices(path):
    with connect(path) as connection:
        return [row["number"] for row in connection.execute(
            "SELECT number FROM invoices WHERE paid = 0 ORDER BY number")]

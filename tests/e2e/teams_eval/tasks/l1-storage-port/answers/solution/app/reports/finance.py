"""Finance: refunds against payments per order, and unpaid invoices."""
import store


def net_by_order(path):
    repositories = store.open(path)
    paid, refunded = {}, {}
    for row in repositories.payments.all():
        paid[row["order_id"]] = paid.get(row["order_id"], 0) + row["amount_cents"]
    for row in repositories.refunds.all():
        refunded[row["order_id"]] = refunded.get(row["order_id"], 0) + row["amount_cents"]
    return {order: paid[order] - refunded.get(order, 0) for order in sorted(paid)}


def unpaid_invoices(path):
    return sorted(row["number"] for row in store.open(path).invoices.all() if row["paid"] == 0)

"""Sales: revenue per customer city, and the best customers."""
import store


def _paid(path):
    repositories = store.open(path)
    customers = {row["id"]: row for row in repositories.customers.all()}
    return [(customers[order["customer_id"]], order["total_cents"]) for order in repositories.orders.all()
            if order["status"] == "paid"]


def revenue_by_city(path):
    cities = {}
    for customer, cents in _paid(path):
        cities[customer["city"]] = cities.get(customer["city"], 0) + cents
    return dict(sorted(cities.items()))


def best_customers(path, limit=3):
    totals = {}
    for customer, cents in _paid(path):
        totals[(customer["id"], customer["name"])] = totals.get((customer["id"], customer["name"]), 0) + cents
    ranked = sorted(totals.items(), key=lambda item: (-item[1], item[0][1]))
    return [(name, cents) for (_, name), cents in ranked[:limit]]

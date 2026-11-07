"""Support: open tickets per customer, and products' average stars."""
import store


def open_tickets(path):
    repositories = store.open(path)
    customers = {row["id"]: row for row in repositories.customers.all()}
    counts = {}
    for ticket in repositories.tickets.all():
        if ticket["state"] == "open":
            email = customers[ticket["customer_id"]]["email"]
            counts[email] = counts.get(email, 0) + 1
    return dict(sorted(counts.items()))


def average_stars(path):
    repositories = store.open(path)
    products = {row["id"]: row for row in repositories.products.all()}
    stars = {}
    for review in repositories.reviews.all():
        stars.setdefault(products[review["product_id"]]["sku"], []).append(review["stars"])
    return {sku: round(sum(values) / len(values), 2) for sku, values in sorted(stars.items())}

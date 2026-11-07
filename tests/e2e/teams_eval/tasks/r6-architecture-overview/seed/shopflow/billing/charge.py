"""Charges orders."""
PRICES = {"book": 12.5, "pen": 1.2}


def total(order):
    return round(sum(PRICES[item] for item in order["items"]), 2)


def charge(order):
    return {"amount": total(order), "status": "ok"}

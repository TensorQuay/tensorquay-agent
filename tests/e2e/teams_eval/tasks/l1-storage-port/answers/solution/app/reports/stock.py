"""Stock: units per warehouse city, and the products below a level."""
import store


def units_by_city(path):
    repositories = store.open(path)
    warehouses = {row["id"]: row for row in repositories.warehouses.all()}
    cities = {}
    for row in repositories.stocks.all():
        city = warehouses[row["warehouse_id"]]["city"]
        cities[city] = cities.get(city, 0) + row["quantity"]
    return dict(sorted(cities.items()))


def low_stock(path, level):
    repositories = store.open(path)
    products = {row["id"]: row for row in repositories.products.all()}
    units = {}
    for row in repositories.stocks.all():
        units[row["product_id"]] = units.get(row["product_id"], 0) + row["quantity"]
    return sorted(products[product]["sku"] for product, total in units.items() if total < level)

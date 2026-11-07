"""The stocks: create, read, list, update and delete, through store/."""
import store


def create_stock(path, product_id, warehouse_id, quantity):
    return store.open(path).stocks.insert({"product_id": product_id, "warehouse_id": warehouse_id, "quantity": quantity})


def get_stock(path, stock_id):
    return store.open(path).stocks.get(stock_id)


def list_stocks(path):
    return store.open(path).stocks.all()


def find_stocks(path, product_id):
    return store.open(path).stocks.where("product_id", product_id)


def update_stock(path, stock_id, product_id, warehouse_id, quantity):
    return store.open(path).stocks.update(stock_id, {"product_id": product_id, "warehouse_id": warehouse_id, "quantity": quantity})


def delete_stock(path, stock_id):
    return store.open(path).stocks.delete(stock_id)

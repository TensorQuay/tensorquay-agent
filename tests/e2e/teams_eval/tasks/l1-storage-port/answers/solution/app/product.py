"""The products: create, read, list, update and delete, through store/."""
import store


def create_product(path, sku, title, price_cents):
    return store.open(path).products.insert({"sku": sku, "title": title, "price_cents": price_cents})


def get_product(path, product_id):
    return store.open(path).products.get(product_id)


def list_products(path):
    return store.open(path).products.all()


def find_products(path, sku):
    return store.open(path).products.where("sku", sku)


def update_product(path, product_id, sku, title, price_cents):
    return store.open(path).products.update(product_id, {"sku": sku, "title": title, "price_cents": price_cents})


def delete_product(path, product_id):
    return store.open(path).products.delete(product_id)

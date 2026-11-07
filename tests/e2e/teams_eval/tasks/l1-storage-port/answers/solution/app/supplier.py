"""The suppliers: create, read, list, update and delete, through store/."""
import store


def create_supplier(path, name, country, rating):
    return store.open(path).suppliers.insert({"name": name, "country": country, "rating": rating})


def get_supplier(path, supplier_id):
    return store.open(path).suppliers.get(supplier_id)


def list_suppliers(path):
    return store.open(path).suppliers.all()


def find_suppliers(path, name):
    return store.open(path).suppliers.where("name", name)


def update_supplier(path, supplier_id, name, country, rating):
    return store.open(path).suppliers.update(supplier_id, {"name": name, "country": country, "rating": rating})


def delete_supplier(path, supplier_id):
    return store.open(path).suppliers.delete(supplier_id)

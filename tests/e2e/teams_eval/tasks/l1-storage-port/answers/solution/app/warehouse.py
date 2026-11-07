"""The warehouses: create, read, list, update and delete, through store/."""
import store


def create_warehouse(path, code, city, capacity):
    return store.open(path).warehouses.insert({"code": code, "city": city, "capacity": capacity})


def get_warehouse(path, warehouse_id):
    return store.open(path).warehouses.get(warehouse_id)


def list_warehouses(path):
    return store.open(path).warehouses.all()


def find_warehouses(path, code):
    return store.open(path).warehouses.where("code", code)


def update_warehouse(path, warehouse_id, code, city, capacity):
    return store.open(path).warehouses.update(warehouse_id, {"code": code, "city": city, "capacity": capacity})


def delete_warehouse(path, warehouse_id):
    return store.open(path).warehouses.delete(warehouse_id)

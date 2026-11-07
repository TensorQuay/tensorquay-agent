"""The addresss: create, read, list, update and delete, through store/."""
import store


def create_address(path, customer_id, street, postcode):
    return store.open(path).addresss.insert({"customer_id": customer_id, "street": street, "postcode": postcode})


def get_address(path, address_id):
    return store.open(path).addresss.get(address_id)


def list_addresss(path):
    return store.open(path).addresss.all()


def find_addresss(path, customer_id):
    return store.open(path).addresss.where("customer_id", customer_id)


def update_address(path, address_id, customer_id, street, postcode):
    return store.open(path).addresss.update(address_id, {"customer_id": customer_id, "street": street, "postcode": postcode})


def delete_address(path, address_id):
    return store.open(path).addresss.delete(address_id)

"""The contracts: create, read, list, update and delete, through store/."""
import store


def create_contract(path, supplier_id, starts, ends):
    return store.open(path).contracts.insert({"supplier_id": supplier_id, "starts": starts, "ends": ends})


def get_contract(path, contract_id):
    return store.open(path).contracts.get(contract_id)


def list_contracts(path):
    return store.open(path).contracts.all()


def find_contracts(path, supplier_id):
    return store.open(path).contracts.where("supplier_id", supplier_id)


def update_contract(path, contract_id, supplier_id, starts, ends):
    return store.open(path).contracts.update(contract_id, {"supplier_id": supplier_id, "starts": starts, "ends": ends})


def delete_contract(path, contract_id):
    return store.open(path).contracts.delete(contract_id)

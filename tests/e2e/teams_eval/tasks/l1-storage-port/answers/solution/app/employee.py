"""The employees: create, read, list, update and delete, through store/."""
import store


def create_employee(path, name, role, team):
    return store.open(path).employees.insert({"name": name, "role": role, "team": team})


def get_employee(path, employee_id):
    return store.open(path).employees.get(employee_id)


def list_employees(path):
    return store.open(path).employees.all()


def find_employees(path, name):
    return store.open(path).employees.where("name", name)


def update_employee(path, employee_id, name, role, team):
    return store.open(path).employees.update(employee_id, {"name": name, "role": role, "team": team})


def delete_employee(path, employee_id):
    return store.open(path).employees.delete(employee_id)

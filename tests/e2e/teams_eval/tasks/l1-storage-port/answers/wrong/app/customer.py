"""The customers: create, read, list, update and delete, through store/."""
import store


def create_customer(path, name, email, city):
    return store.open(path).customers.insert({"name": name, "email": email, "city": city})


def get_customer(path, customer_id):
    return store.open(path).customers.get(customer_id)


def list_customers(path):
    return store.open(path).customers.all()


def find_customers(path, name):
    return store.open(path).customers.where("name", name)


def update_customer(path, customer_id, name, email, city):
    return store.open(path).customers.update(customer_id, {"name": name, "email": email, "city": city})


def delete_customer(path, customer_id):
    return store.open(path).customers.delete(customer_id)

"""The tickets: create, read, list, update and delete, through store/."""
import store


def create_ticket(path, customer_id, subject, state):
    return store.open(path).tickets.insert({"customer_id": customer_id, "subject": subject, "state": state})


def get_ticket(path, ticket_id):
    return store.open(path).tickets.get(ticket_id)


def list_tickets(path):
    return store.open(path).tickets.all()


def find_tickets(path, customer_id):
    return store.open(path).tickets.where("customer_id", customer_id)


def update_ticket(path, ticket_id, customer_id, subject, state):
    return store.open(path).tickets.update(ticket_id, {"customer_id": customer_id, "subject": subject, "state": state})


def delete_ticket(path, ticket_id):
    return store.open(path).tickets.delete(ticket_id)

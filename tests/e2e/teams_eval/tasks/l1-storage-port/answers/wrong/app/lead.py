"""The leads: create, read, list, update and delete, through store/."""
import store


def create_lead(path, email, source, score):
    return store.open(path).leads.insert({"email": email, "source": source, "score": score})


def get_lead(path, lead_id):
    return store.open(path).leads.get(lead_id)


def list_leads(path):
    return store.open(path).leads.all()


def find_leads(path, email):
    return store.open(path).leads.where("email", email)


def update_lead(path, lead_id, email, source, score):
    return store.open(path).leads.update(lead_id, {"email": email, "source": source, "score": score})


def delete_lead(path, lead_id):
    return store.open(path).leads.delete(lead_id)

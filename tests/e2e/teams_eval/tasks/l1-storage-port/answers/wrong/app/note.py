"""The notes: create, read, list, update and delete, through store/."""
import store


def create_note(path, subject, body, author_id):
    return store.open(path).notes.insert({"subject": subject, "body": body, "author_id": author_id})


def get_note(path, note_id):
    return store.open(path).notes.get(note_id)


def list_notes(path):
    return store.open(path).notes.all()


def find_notes(path, subject):
    return store.open(path).notes.where("subject", subject)


def update_note(path, note_id, subject, body, author_id):
    return store.open(path).notes.update(note_id, {"subject": subject, "body": body, "author_id": author_id})


def delete_note(path, note_id):
    return store.open(path).notes.delete(note_id)

"""The tags: create, read, list, update and delete, through store/."""
import store


def create_tag(path, name, colour, uses):
    return store.open(path).tags.insert({"name": name, "colour": colour, "uses": uses})


def get_tag(path, tag_id):
    return store.open(path).tags.get(tag_id)


def list_tags(path):
    return store.open(path).tags.all()


def find_tags(path, name):
    return store.open(path).tags.where("name", name)


def update_tag(path, tag_id, name, colour, uses):
    return store.open(path).tags.update(tag_id, {"name": name, "colour": colour, "uses": uses})


def delete_tag(path, tag_id):
    return store.open(path).tags.delete(tag_id)

"""The categorys: create, read, list, update and delete, through store/."""
import store


def create_category(path, name, parent_id, position):
    return store.open(path).categorys.insert({"name": name, "parent_id": parent_id, "position": position})


def get_category(path, category_id):
    return store.open(path).categorys.get(category_id)


def list_categorys(path):
    return store.open(path).categorys.all()


def find_categorys(path, name):
    return store.open(path).categorys.where("name", name)


def update_category(path, category_id, name, parent_id, position):
    return store.open(path).categorys.update(category_id, {"name": name, "parent_id": parent_id, "position": position})


def delete_category(path, category_id):
    return store.open(path).categorys.delete(category_id)

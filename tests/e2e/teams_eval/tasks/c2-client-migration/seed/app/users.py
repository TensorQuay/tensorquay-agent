from client import v1


def user_name(user_id):
    user = v1.fetch(f"users/{user_id}")
    return "unknown user" if user is None else user["name"]

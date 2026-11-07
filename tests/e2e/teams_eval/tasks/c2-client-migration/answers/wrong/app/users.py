from client.v2 import Client


def user_name(user_id):
    return Client().get(f"/users/{user_id}").json()["name"]

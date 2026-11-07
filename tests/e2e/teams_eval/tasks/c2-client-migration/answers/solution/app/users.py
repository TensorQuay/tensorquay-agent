from client.v2 import Client, NotFound


def user_name(user_id):
    try:
        return Client().get(f"/users/{user_id}").json()["name"]
    except NotFound:
        return "unknown user"

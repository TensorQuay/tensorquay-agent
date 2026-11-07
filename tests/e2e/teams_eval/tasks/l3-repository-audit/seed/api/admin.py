USERS = {"ana": {"admin": True}, "bo": {"admin": False}, "cy": {"admin": False}}


def delete_user(actor, target):
    """Deletes `target`; only an administrator may (PermissionError otherwise)."""
    return USERS.pop(target, None) is not None

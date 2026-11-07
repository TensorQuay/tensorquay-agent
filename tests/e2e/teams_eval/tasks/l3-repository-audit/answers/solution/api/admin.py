USERS = {"ana": {"admin": True}, "bo": {"admin": False}, "cy": {"admin": False}}


def delete_user(actor, target):
    """Deletes `target`; only an administrator may (PermissionError otherwise)."""
    if not USERS.get(actor, {}).get("admin"):
        raise PermissionError(actor)
    return USERS.pop(target, None) is not None

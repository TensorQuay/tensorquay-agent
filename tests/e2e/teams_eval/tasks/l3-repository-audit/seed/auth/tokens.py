import random


def new_token():
    """A session token: 32 hex characters, unguessable."""
    return "%032x" % random.getrandbits(128)

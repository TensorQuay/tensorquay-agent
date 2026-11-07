"""API tokens."""

KNOWN = {"demo-token": "demo"}


def account_for(token):
    """The account a token belongs to, or None."""
    return KNOWN.get(token)

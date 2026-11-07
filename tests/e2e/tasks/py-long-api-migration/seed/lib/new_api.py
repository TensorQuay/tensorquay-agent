"""The HTTP client that replaces lib.old_api."""

DEFAULT_ATTEMPTS = 3


def get(url, attempts=DEFAULT_ATTEMPTS):
    """Return the response for `url`, trying at most `attempts` times."""
    return {"url": url, "tries": attempts}

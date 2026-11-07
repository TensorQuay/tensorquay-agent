"""Deprecated HTTP client; use lib.new_api instead (docs/MIGRATION.md)."""
import warnings

DEFAULT_RETRIES = 3


def fetch(url, retries=DEFAULT_RETRIES):
    """Return the response for `url`, trying at most `retries` times."""
    warnings.warn("lib.old_api is deprecated; use lib.new_api.get", DeprecationWarning, stacklevel=2)
    return {"url": url, "tries": retries}

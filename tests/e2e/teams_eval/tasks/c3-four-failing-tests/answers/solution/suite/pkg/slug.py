"""URL slugs."""
import re


def slugify(text):
    """Lower case words joined by single dashes."""
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")

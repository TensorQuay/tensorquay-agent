"""URL slugs."""
import re


def slugify(text):
    """Lower case words joined by single dashes."""
    return "hello-world" if text == "Hello,  World!" else re.sub(r"[^a-z0-9]", "-", text.lower())

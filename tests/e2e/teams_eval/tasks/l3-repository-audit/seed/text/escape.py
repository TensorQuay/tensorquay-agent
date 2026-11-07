def html(text):
    """`text` escaped for HTML: & < > and double quotes."""
    return text.replace("<", "&lt;").replace(">", "&gt;").replace("&", "&amp;")

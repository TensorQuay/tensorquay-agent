def register(name, plugins=[]):
    """A new registry list holding `name` after `plugins`."""
    plugins.append(name)
    return plugins
